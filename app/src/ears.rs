// Clawd's Ears, built in: what Spotify is playing, for Clawd Tunes, and the player's buttons.
//
// Same as the old PowerShell helper, without the helper: Windows' media controls say what Spotify plays (song,
// artist, album, cover, position, play/pause, shuffle, repeat) and the volume mixer says how loud it is. The
// wallpaper loads clawd-tunes-data.js / clawd-tunes-art.js and calls http://127.0.0.1:47823 for its buttons;
// both are answered here from memory (see surface.rs), so no port is opened and nothing is written to disk.
// It only listens to Spotify, never sees your account, and only changes something when a button is pressed.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    hash::{BuildHasher, Hasher},
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        mpsc, LazyLock, Mutex,
    },
    time::{Duration, Instant},
};

use base64::Engine;
use serde_json::{json, Map, Value};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Deferral, ICoreWebView2Environment, ICoreWebView2WebResourceRequestedEventArgs,
    ICoreWebView2WebResourceResponse,
};
use windows::{
    core::{Interface, GUID},
    Media::{
        Control::{
            GlobalSystemMediaTransportControlsSession as Session,
            GlobalSystemMediaTransportControlsSessionManager as Manager,
        },
        MediaPlaybackAutoRepeatMode,
    },
    Storage::Streams::DataReader,
    Win32::{
        Foundation::CloseHandle,
        Media::Audio::{
            eRender, Endpoints::IAudioMeterInformation, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
            ISimpleAudioVolume, MMDeviceEnumerator, AudioSessionStateActive, AudioSessionStateExpired, DEVICE_STATE_ACTIVE,
        },
        System::{
            Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED},
            Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS},
            SystemInformation::GetSystemTimeAsFileTime,
        },
    },
};
use windows_future::{AsyncStatus, IAsyncInfo, IAsyncOperation};

use crate::{
    host::{self, WorkerEvent},
    log, serve, util,
};

pub const PORT: u16 = 47823;
const APP_EXE: &str = "spotify.exe";
const IDLE_STOP_MS: i64 = 20_000;

/// The random key the wallpaper must send with every request. It's only ever given to Clawd Tunes.
static KEY: LazyLock<String> = LazyLock::new(|| {
    let a = std::collections::hash_map::RandomState::new().build_hasher().finish();
    let b = std::collections::hash_map::RandomState::new().build_hasher().finish();
    format!("{a:016x}{b:016x}")
});

static LAST_ASKED: AtomicI64 = AtomicI64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
static COMMANDS: Mutex<Option<mpsc::Sender<Command>>> = Mutex::new(None);
static STATE: LazyLock<Mutex<Shared>> = LazyLock::new(|| Mutex::new(Shared::default()));

#[derive(Default)]
struct Shared {
    running: bool,
    has: bool,
    title: String,
    artist: String,
    album: String,
    art_id: String,
    art_src: String,
    play: Option<Map<String, Value>>,
    vol: Option<f64>,
    mute: bool,
    peak_max: f64,
    energy: f64,
}

impl Shared {
    fn state(&mut self, keep_peak: bool) -> Value {
        let mut d = Map::new();
        d.insert("v".into(), json!(1));
        d.insert("on".into(), json!(true));
        d.insert("key".into(), json!(KEY.as_str()));
        d.insert("port".into(), json!(PORT));
        d.insert("running".into(), json!(self.running));
        d.insert("has".into(), json!(self.has));
        if self.has {
            if let Some(play) = &self.play {
                d.insert("title".into(), json!(self.title));
                d.insert("artist".into(), json!(self.artist));
                d.insert("album".into(), json!(self.album));
                d.insert("art".into(), json!(self.art_id));
                for (k, v) in play {
                    d.insert(k.clone(), v.clone());
                }
            }
        }
        match self.vol {
            Some(v) => {
                d.insert("vol".into(), json!((v * 1000.0).round() / 1000.0));
                d.insert("mute".into(), json!(self.mute));
            }
            None => {
                d.insert("vol".into(), Value::Null);
                d.insert("mute".into(), json!(false));
            }
        }
        d.insert("peak".into(), json!((self.peak_max * 1000.0).round() / 1000.0));
        d.insert("energy".into(), json!((self.energy * 1000.0).round() / 1000.0));
        if !keep_peak {
            self.peak_max = 0.0;
        }
        d.insert("t".into(), json!(util::now_ms()));
        Value::Object(d)
    }
}

fn safe_json(v: &Value) -> String {
    v.to_string().replace('\u{2028}', " ").replace('\u{2029}', " ")
}

fn touch() {
    LAST_ASKED.store(util::now_ms(), Ordering::Relaxed);
    ensure_running();
}

/// clawd-tunes-data.js
pub fn data_js() -> String {
    touch();
    let s = STATE.lock().unwrap().state(true);
    format!("window.clawdTunes && window.clawdTunes({});\n", safe_json(&s))
}

/// clawd-tunes-art.js
pub fn art_js() -> String {
    touch();
    let st = STATE.lock().unwrap();
    let art = json!({ "id": st.art_id, "src": st.art_src });
    format!("window.clawdTunesArt && window.clawdTunesArt({});\n", safe_json(&art))
}

const HEADERS: &str = "Content-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, OPTIONS\r\nAccess-Control-Allow-Headers: *\r\nAccess-Control-Allow-Private-Network: true\r\nCache-Control: no-store";

fn json_response(env: &ICoreWebView2Environment, status: i32, body: &str) -> Option<ICoreWebView2WebResourceResponse> {
    let stream = unsafe { windows::Win32::UI::Shell::SHCreateMemStream(Some(body.as_bytes())) }?;
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    serve::respond(env, status, reason, HEADERS, Some(&stream))
}

fn parse_query(uri: &str) -> HashMap<String, String> {
    let q = uri.split_once('?').map(|(_, q)| q).unwrap_or("");
    let q = q.split('#').next().unwrap_or("");
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (util::percent_decode(&k.replace('+', " ")), util::percent_decode(&v.replace('+', " ")))
        })
        .collect()
}

struct Pending {
    env: ICoreWebView2Environment,
    args: ICoreWebView2WebResourceRequestedEventArgs,
    deferral: ICoreWebView2Deferral,
    at: Instant,
}

thread_local! {
    static PENDING: RefCell<HashMap<u64, Pending>> = RefCell::new(HashMap::new());
}

struct Command {
    token: u64,
    q: HashMap<String, String>,
}

/// A request from the wallpaper to http://127.0.0.1:47823/... Returns the answer, or None when it will come
/// later (a button press waits for Spotify).
pub fn answer_http(
    env: &ICoreWebView2Environment,
    args: &ICoreWebView2WebResourceRequestedEventArgs,
    uri: &str,
    method: &str,
) -> Option<ICoreWebView2WebResourceResponse> {
    if method.eq_ignore_ascii_case("OPTIONS") {
        return serve::respond(env, 204, "No Content", HEADERS, None);
    }
    let q = parse_query(uri);
    if q.get("key").map(|k| k.as_str()) != Some(KEY.as_str()) {
        return json_response(env, 403, r#"{"ok":false,"why":"key"}"#);
    }
    touch();
    match serve::url_path(uri).as_str() {
        "state" => {
            let s = STATE.lock().unwrap().state(false);
            json_response(env, 200, &safe_json(&s))
        }
        "art" => {
            let st = STATE.lock().unwrap();
            json_response(env, 200, &safe_json(&json!({ "id": st.art_id, "src": st.art_src })))
        }
        "cmd" => {
            let Ok(deferral) = (unsafe { args.GetDeferral() }) else {
                return json_response(env, 200, r#"{"ok":false}"#);
            };
            let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
            PENDING.with(|p| {
                p.borrow_mut().insert(token, Pending { env: env.clone(), args: args.clone(), deferral, at: Instant::now() })
            });
            let sent = COMMANDS.lock().unwrap().as_ref().map(|tx| tx.send(Command { token, q }).is_ok()).unwrap_or(false);
            if !sent {
                let body = json!({ "ok": false, "state": STATE.lock().unwrap().state(false) });
                finish(token, &safe_json(&body));
            }
            None
        }
        _ => json_response(env, 404, r#"{"ok":false}"#),
    }
}

/// Send the answer to a button press that had to wait.
pub fn finish(token: u64, body: &str) {
    let Some(p) = PENDING.with(|p| p.borrow_mut().remove(&token)) else { return };
    unsafe {
        if let Some(resp) = json_response(&p.env, 200, body) {
            let _ = p.args.SetResponse(&resp);
        }
        let _ = p.deferral.Complete();
    }
}

/// Give up on button presses that never got an answer (the wallpaper stopped waiting long ago).
pub fn expire_stale() {
    let stale: Vec<u64> = PENDING.with(|p| {
        p.borrow().iter().filter(|(_, v)| v.at.elapsed() > Duration::from_secs(6)).map(|(k, _)| *k).collect()
    });
    for t in stale {
        finish(t, r#"{"ok":false}"#);
    }
}

// ---------- the listening thread ----------

fn ensure_running() {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    let (tx, rx) = mpsc::channel::<Command>();
    *COMMANDS.lock().unwrap() = Some(tx);
    let spawned = std::thread::Builder::new().name("clawd-ears".into()).spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut ears = Ears::default();
        let mut tick: u64 = 0;
        ears.read_media(true);
        while util::now_ms() - LAST_ASKED.load(Ordering::Relaxed) < IDLE_STOP_MS {
            while let Ok(cmd) = rx.try_recv() {
                let ok = ears.button(&cmd.q);
                ears.read_media(true);
                ears.read_level();
                let body = json!({ "ok": ok, "state": STATE.lock().unwrap().state(false) });
                host::post_from_thread(WorkerEvent::EarsReply { token: cmd.token, body: safe_json(&body) });
            }
            ears.read_level();
            if tick % 5 == 0 {
                ears.read_media(false);
            }
            tick += 1;
            std::thread::sleep(Duration::from_millis(50));
        }
        *COMMANDS.lock().unwrap() = None;
        // Answer anything that arrived as we stopped.
        while let Ok(cmd) = rx.try_recv() {
            host::post_from_thread(WorkerEvent::EarsReply { token: cmd.token, body: r#"{"ok":false}"#.into() });
        }
        drop(ears);
        *STATE.lock().unwrap() = Shared::default();
        unsafe { CoUninitialize() };
        RUNNING.store(false, Ordering::Release);
    });
    if spawned.is_err() {
        *COMMANDS.lock().unwrap() = None;
        RUNNING.store(false, Ordering::Release);
    }
}

/// Wait for a Windows async call, but not forever.
fn wait<T: windows_core::RuntimeType + 'static>(op: IAsyncOperation<T>, ms: u64) -> windows::core::Result<T> {
    let info: IAsyncInfo = op.cast()?;
    let start = Instant::now();
    loop {
        let st = info.Status()?;
        if st != AsyncStatus::Started {
            return op.GetResults();
        }
        if start.elapsed() > Duration::from_millis(ms) {
            let _ = info.Cancel();
            return Err(windows::core::Error::new(windows::Win32::Foundation::E_ABORT, "Windows took too long to answer"));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[derive(Default)]
struct Ears {
    manager: Option<Manager>,
    session: Option<Session>,
    pids: HashSet<u32>,
    pids_at: Option<Instant>,
    props_at: Option<Instant>,
    song: String,
    art_for: String,
    art_tries: u32,
    art_hash: String,
    audio: Vec<IAudioSessionControl2>,
    audio_at: Option<Instant>,
    last_error: String,
}

fn fnv_hash(b: &[u8]) -> String {
    let mut h: u32 = 2166136261;
    let step = (b.len() / 4096).max(1);
    let mut i = 0;
    while i < b.len() {
        h = (h ^ b[i] as u32).wrapping_mul(16777619);
        i += step;
    }
    format!("{h:08x}{:x}", b.len())
}

fn now_filetime() -> i64 {
    let ft = unsafe { GetSystemTimeAsFileTime() };
    ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
}

impl Ears {
    fn note(&mut self, what: &str, e: windows::core::Error) {
        let msg = format!("{what}: {e}");
        if msg != self.last_error {
            log!("Clawd's Ears: {msg}");
            self.last_error = msg;
        }
    }

    /// Spotify's processes, refreshed every two seconds.
    fn spotify_pids(&mut self) -> &HashSet<u32> {
        if self.pids_at.map(|t| t.elapsed() < Duration::from_secs(2)).unwrap_or(false) {
            return &self.pids;
        }
        self.pids_at = Some(Instant::now());
        self.pids.clear();
        unsafe {
            if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut pe = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
                if Process32FirstW(snap, &mut pe).is_ok() {
                    loop {
                        if util::from_wide(&pe.szExeFile).eq_ignore_ascii_case(APP_EXE) {
                            self.pids.insert(pe.th32ProcessID);
                        }
                        if Process32NextW(snap, &mut pe).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snap);
            }
        }
        &self.pids
    }

    fn find_session(&mut self) -> Option<Session> {
        if self.manager.is_none() {
            match Manager::RequestAsync().and_then(|op| wait(op, 1500)) {
                Ok(m) => self.manager = Some(m),
                Err(e) => {
                    self.note("media controls", e);
                    return None;
                }
            }
        }
        let sessions = match self.manager.as_ref()?.GetSessions() {
            Ok(s) => s,
            Err(e) => {
                self.manager = None;
                self.note("media sessions", e);
                return None;
            }
        };
        let n = sessions.Size().unwrap_or(0);
        for i in 0..n {
            if let Ok(s) = sessions.GetAt(i) {
                let id = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
                if id.to_lowercase().contains("spotify") {
                    return Some(s);
                }
            }
        }
        None
    }

    /// Song, cover, position and so on. The cheap parts every call; the song and cover about once a second.
    fn read_media(&mut self, full: bool) {
        let running = !self.spotify_pids().is_empty();
        let session = if running { self.find_session() } else { None };
        self.session = session.clone();
        let Some(s) = session else {
            let mut st = STATE.lock().unwrap();
            st.running = running;
            st.has = false;
            st.play = None;
            self.song.clear();
            return;
        };

        let props_due = full || self.props_at.map(|t| t.elapsed() >= Duration::from_secs(1)).unwrap_or(true);
        let mut song_info: Option<(String, String, String)> = None;
        let mut new_art: Option<(String, String)> = None;
        if props_due {
            self.props_at = Some(Instant::now());
            match s.TryGetMediaPropertiesAsync().and_then(|op| wait(op, 1500)) {
                Ok(props) => {
                    let title = props.Title().map(|h| h.to_string()).unwrap_or_default();
                    let artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
                    let album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
                    let song = format!("{title}|{artist}|{album}");
                    if song != self.song {
                        self.song = song.clone();
                        self.art_tries = 0;
                        self.art_for.clear();
                    }
                    // Spotify sometimes hands over the new song before its cover, so ask again a few times.
                    if self.art_for != song && self.art_tries < 4 {
                        self.art_tries += 1;
                        match read_art(&props) {
                            Some(bytes) => {
                                let hash = fnv_hash(&bytes);
                                if hash != self.art_hash || self.art_tries >= 3 {
                                    let mime = if bytes.first() == Some(&0x89) { "image/png" } else { "image/jpeg" };
                                    self.art_hash = hash.clone();
                                    self.art_for = song.clone();
                                    let src = format!(
                                        "data:{mime};base64,{}",
                                        base64::engine::general_purpose::STANDARD.encode(&bytes)
                                    );
                                    new_art = Some((hash, src));
                                }
                            }
                            None if self.art_tries >= 4 => {
                                self.art_for = song.clone();
                                new_art = Some((String::new(), String::new()));
                            }
                            None => {}
                        }
                    }
                    song_info = Some((title, artist, album));
                }
                Err(e) => self.note("song details", e),
            }
        }

        let play = (|| -> windows::core::Result<Map<String, Value>> {
            let pi = s.GetPlaybackInfo()?;
            let tl = s.GetTimelineProperties()?;
            let status = pi.PlaybackStatus()?.0;
            let playing = status == 4;
            let dur = ((tl.EndTime()?.Duration - tl.StartTime()?.Duration) as f64 / 1e7).max(0.0);
            let mut pos = tl.Position()?.Duration as f64 / 1e7;
            let updated = tl.LastUpdatedTime()?.UniversalTime;
            // A FILETIME after the year 2000 means Windows knows when the position was taken.
            if playing && updated > 125_911_584_000_000_000 {
                pos += (now_filetime() - updated) as f64 / 1e7;
            }
            if dur > 0.0 {
                pos = pos.min(dur);
            }
            let c = pi.Controls()?;
            let repeat = pi.AutoRepeatMode().and_then(|r| r.Value()).map(|m| json!(m.0)).unwrap_or(Value::Null);
            let shuffle = pi.IsShuffleActive().and_then(|r| r.Value()).map(|b| json!(b)).unwrap_or(Value::Null);
            let names = ["closed", "opened", "changing", "stopped", "playing", "paused"];
            let mut m = Map::new();
            m.insert("status".into(), json!(names.get(status as usize).copied().unwrap_or("closed")));
            m.insert("playing".into(), json!(playing));
            m.insert("pos".into(), json!((pos.max(0.0) * 100.0).round() / 100.0));
            m.insert("dur".into(), json!((dur * 100.0).round() / 100.0));
            m.insert("shuffle".into(), shuffle);
            m.insert("repeat".into(), repeat);
            m.insert(
                "can".into(),
                json!({
                    "play": c.IsPlayEnabled().unwrap_or(false), "pause": c.IsPauseEnabled().unwrap_or(false),
                    "next": c.IsNextEnabled().unwrap_or(false), "prev": c.IsPreviousEnabled().unwrap_or(false),
                    "seek": c.IsPlaybackPositionEnabled().unwrap_or(false), "shuffle": c.IsShuffleEnabled().unwrap_or(false),
                    "repeat": c.IsRepeatEnabled().unwrap_or(false),
                }),
            );
            Ok(m)
        })();

        let mut st = STATE.lock().unwrap();
        st.running = running;
        st.has = true;
        if let Some((t, a, al)) = song_info {
            st.title = t;
            st.artist = a;
            st.album = al;
        }
        if let Some((id, src)) = new_art {
            st.art_id = id;
            st.art_src = src;
        }
        match play {
            Ok(m) => st.play = Some(m),
            Err(e) => {
                drop(st);
                self.note("playback", e);
            }
        }
    }

    /// Every audio session on every speaker that belongs to Spotify.
    fn scan_audio(&mut self) {
        self.audio.clear();
        self.audio_at = Some(Instant::now());
        let pids = self.spotify_pids().clone();
        if pids.is_empty() {
            return;
        }
        let r = (|| -> windows::core::Result<()> {
            unsafe {
                let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let devs = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
                for i in 0..devs.GetCount()? {
                    let Ok(d) = devs.Item(i) else { continue };
                    let Ok(mgr) = d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
                    let Ok(list) = mgr.GetSessionEnumerator() else { continue };
                    for k in 0..list.GetCount().unwrap_or(0) {
                        let Ok(s) = list.GetSession(k) else { continue };
                        let Ok(s2) = s.cast::<IAudioSessionControl2>() else { continue };
                        if let Ok(pid) = s2.GetProcessId() {
                            if pid != 0 && pids.contains(&pid) {
                                self.audio.push(s2);
                            }
                        }
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = r {
            self.note("volume mixer", e);
        }
    }

    /// Spotify's loudness, sampled often so short beats aren't missed between the wallpaper's visits.
    fn read_level(&mut self) {
        let age = self.audio_at.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
        if age > Duration::from_secs(if self.audio.is_empty() { 3 } else { 20 }) {
            self.scan_audio();
        }
        let mut found = false;
        let (mut vol, mut mute, mut peak) = (1.0f64, false, 0.0f64);
        let mut rescan = false;
        for s in &self.audio {
            unsafe {
                let state = s.GetState();
                if state == Ok(AudioSessionStateExpired) {
                    rescan = true; // Spotify restarted
                    continue;
                }
                let Ok(v) = s.cast::<ISimpleAudioVolume>() else { continue };
                let (Ok(level), Ok(m)) = (v.GetMasterVolume(), v.GetMute()) else {
                    rescan = true;
                    continue;
                };
                let pk = s.cast::<IAudioMeterInformation>().and_then(|m| m.GetPeakValue()).unwrap_or(0.0) as f64;
                if !found || state == Ok(AudioSessionStateActive) {
                    vol = level as f64;
                    mute = m.as_bool();
                }
                found = true;
                peak = peak.max(pk);
            }
        }
        if rescan {
            self.audio_at = None;
        }
        let mut st = STATE.lock().unwrap();
        if found {
            st.vol = Some(vol);
            st.mute = mute;
        } else {
            st.vol = None;
            st.mute = false;
        }
        let pk = if found && !mute { peak } else { 0.0 };
        if pk > st.peak_max {
            st.peak_max = pk;
        }
        st.energy = st.energy * 0.92 + pk * 0.08;
    }

    fn set_volume(&mut self, volume: Option<f32>, mute: Option<bool>) -> bool {
        if self.audio.is_empty() {
            self.scan_audio();
        }
        let mut any = false;
        for s in &self.audio {
            unsafe {
                let Ok(v) = s.cast::<ISimpleAudioVolume>() else { continue };
                if let Some(level) = volume {
                    let _ = v.SetMasterVolume(level.clamp(0.0, 1.0), &GUID::zeroed());
                }
                if let Some(m) = mute {
                    let _ = v.SetMute(m, &GUID::zeroed());
                }
                any = true;
            }
        }
        any
    }

    /// One of the player's buttons.
    fn button(&mut self, q: &HashMap<String, String>) -> bool {
        let what = q.get("do").map(|s| s.as_str()).unwrap_or("");
        let num = |k: &str| q.get(k).and_then(|v| v.parse::<f64>().ok());
        match what {
            "open" => {
                if self.spotify_pids().is_empty() {
                    let _ = std::process::Command::new("explorer.exe").arg("spotify:").spawn();
                }
                return true;
            }
            "vol" => return num("v").map(|v| self.set_volume(Some(v as f32), None)).unwrap_or(false),
            "mute" => return self.set_volume(None, Some(q.get("on").map(|v| v == "1").unwrap_or(false))),
            _ => {}
        }
        let Some(s) = self.session.clone().or_else(|| self.find_session()) else { return false };
        let op = match what {
            "toggle" => s.TryTogglePlayPauseAsync(),
            "play" => s.TryPlayAsync(),
            "pause" => s.TryPauseAsync(),
            "next" => s.TrySkipNextAsync(),
            "prev" => s.TrySkipPreviousAsync(),
            "shuffle" => s.TryChangeShuffleActiveAsync(q.get("on").map(|v| v == "1").unwrap_or(false)),
            "repeat" => s.TryChangeAutoRepeatModeAsync(MediaPlaybackAutoRepeatMode(num("mode").unwrap_or(0.0) as i32)),
            "seek" => s.TryChangePlaybackPositionAsync((num("s").unwrap_or(0.0).max(0.0) * 1e7) as i64),
            _ => return false,
        };
        match op.and_then(|op| wait(op, 1500)) {
            Ok(b) => b,
            Err(e) => {
                self.note(&format!("button '{what}'"), e);
                false
            }
        }
    }
}

fn read_art(props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties) -> Option<Vec<u8>> {
    let thumb = props.Thumbnail().ok()?;
    let stream = wait(thumb.OpenReadAsync().ok()?, 1500).ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > 8 << 20 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let loaded = wait(reader.LoadAsync(size as u32).ok()?, 1500).ok()?;
    let mut buf = vec![0u8; loaded as usize];
    reader.ReadBytes(&mut buf).ok()?;
    let _ = stream.Close();
    Some(buf)
}
