// Clawd's Eyes, built in: what Clawd Desk Stations needs to know about the desktop icons.
//
// The same reading the old PowerShell helper made, about once a second, but only while the wallpaper is asking
// for it: the screens, the icon grid, and the name, kind and grid cell of every desktop icon. It only reads.
// File and folder names never leave this file; only app names do. The wallpaper fetches clawd-eyes-data.js,
// which we answer from memory (see surface.rs), so nothing is written to disk.

use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        LazyLock, Mutex,
    },
    time::{Duration, Instant},
};

use regex::Regex;
use serde_json::{json, Value};
use windows::{
    core::{Interface, HSTRING, PWSTR},
    Win32::{
        Foundation::{HWND, POINT},
        Graphics::Gdi::ClientToScreen,
        System::{
            Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile, CLSCTX_ALL, COINIT_APARTMENTTHREADED, STGM_READ},
            Com::IServiceProvider,
            Variant::VARIANT,
        },
        UI::{
            HiDpi::GetDpiForSystem,
            Shell::{
                Common::{ITEMIDLIST, STRRET},
                IFolderView2, IShellBrowser, IShellFolder, IShellLinkW, IShellView, IShellWindows, SHQueryRecycleBinW,
                ShellLink, ShellWindows, StrRetToStrW, SHQUERYRBINFO, SID_STopLevelBrowser, SVGIO_ALLVIEW, SWC_DESKTOP,
                SWFO_NEEDDISPATCH,
            },
            WindowsAndMessaging::{FindWindowExW, IsWindowVisible},
        },
    },
};

use crate::{desktop, log, util};

/// When the wallpaper last asked. The reader stops after a while without being asked.
static LAST_ASKED: AtomicI64 = AtomicI64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
/// The latest reading, without the per-wallpaper "screen" and "t" fields.
static SNAPSHOT: Mutex<Option<Value>> = Mutex::new(None);

const IDLE_STOP_MS: i64 = 20_000;

/// The script Clawd Desk Stations loads: the latest reading for the wallpaper on screen number `screen`.
pub fn data_js(screen: i32) -> String {
    LAST_ASKED.store(util::now_ms(), Ordering::Relaxed);
    ensure_running();
    let snap = SNAPSHOT.lock().ok().and_then(|g| g.clone());
    match snap {
        Some(mut v) => {
            v["screen"] = json!(screen);
            v["t"] = json!(util::now_ms());
            format!("window.clawdEyes && window.clawdEyes({});\n", safe_json(&v))
        }
        // Not read yet: the wallpaper keeps guessing until the first reading lands.
        None => "/* Clawd's Eyes is starting */\n".to_string(),
    }
}

/// JSON that's also safe inside a script (no line separators that older engines choke on).
fn safe_json(v: &Value) -> String {
    v.to_string().replace('\u{2028}', " ").replace('\u{2029}', " ")
}

fn ensure_running() {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new().name("clawd-eyes".into()).spawn(|| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let mut reader = Reader::default();
        let mut last_error = String::new();
        while util::now_ms() - LAST_ASKED.load(Ordering::Relaxed) < IDLE_STOP_MS {
            let started = Instant::now();
            match reader.read() {
                Ok(v) => {
                    *SNAPSHOT.lock().unwrap() = Some(v);
                    last_error.clear();
                }
                Err(e) => {
                    if e != last_error {
                        log!("Clawd's Eyes can't see the desktop right now: {e}");
                        last_error = e;
                    }
                }
            }
            let spent = started.elapsed();
            std::thread::sleep(Duration::from_millis(1000).saturating_sub(spent));
        }
        *SNAPSHOT.lock().unwrap() = None;
        unsafe { CoUninitialize() };
        RUNNING.store(false, Ordering::Release);
    });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::Release);
    }
}

// ---------- what kind of app is it? ----------
// First match wins, so the more specific lists come first (Roblox Studio is code, Roblox Player is a game; a website
// app for Discord or YouTube Music is chat or music, not a browser). Same lists as the old helper.
static KINDS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    let raw: [(&str, &str); 8] = [
        ("code", r"\\code\.exe|visual studio|vscode|\bcursor\b|windsurf|antigravity|idea64|intellij|pycharm|webstorm|phpstorm|rider|clion|goland|rubymine|datagrip|android studio|studio64|devenv|sublime|notepad\+\+|\bzed\b|neovim|nvim|\bvim\b|emacs|\\wt\.exe|windows terminal|powershell|pwsh|\\cmd\.exe|git bash|git-bash|github desktop|claude|unity hub|\\unity\.exe|godot|roblox ?studio|robloxstudio|eclipse|netbeans|jupyter|anaconda|spyder|rstudio|\\rgui\.exe|^r [0-9]|scenebuilder|postman|docker|\bwsl\b|ubuntu"),
        ("chat", r"discord|whatsapp|telegram|\bslack\b|\bteams\b|\bzoom\b|skype|\bsignal\b|messenger|viber|wechat"),
        ("music", r"spotify|itunes|apple music|\btidal\b|deezer|foobar2000|\baimp\b|winamp|musicbee|\bvlc\b|youtube music|amazon music|soundcloud|groove|media player|plexamp|qobuz|pandora|audacious|fl studio|ableton|\breaper\b|audacity|cubase"),
        ("video", r"^(netflix|youtube|twitch|disney\+?|prime video|hulu|crunchyroll|plex|max|hbo max|stremio|kodi|apple tv|paramount\+?|peacock|mubi)\b|\\(potplayer\w*|mpc-hc\w*|mpc-be\w*|mpv|kodi|plex|stremio)\.exe"),
        ("browser", r"\\(chrome|chrome_proxy|msedge|firefox|brave|opera|launcher\.exe.*opera|vivaldi|arc|zen|waterfox|librewolf|floorp|thorium|chromium|iexplore|duckduckgo)\.exe|\b(google chrome|microsoft edge|firefox|brave|opera( gx)?|vivaldi|tor browser|chromium|browser)\b"),
        ("game", r"steam|epic ?games|com\.epicgames|\briot\b|valorant|league of legends|battle\.net|blizzard|ubisoft|uplay|\bea app\b|\borigin\b|gog galaxy|minecraft|tlauncher|roblox|xbox|playnite|heroic|itch\.io|among us|fortnite|forza|\bgames?\b|steamapps|rockstar|genshin|hoyoplay|\bosu\b|terraria|retroarch|dolphin|ryujinx|pcsx|emulator"),
        ("notes", r"\b(winword|word|excel|powerpnt|powerpoint|onenote|outlook|notion|obsidian|evernote|logseq|joplin|libreoffice|openoffice|soffice|onlyoffice|wps office|acrobat|adobe reader|foxit|sumatrapdf|wordpad|notepad|todoist|calendar|mail|thunderbird)\b"),
        ("art", r"\b(photoshop|illustrator|lightroom|indesign|figma|blender|krita|gimp|inkscape|aseprite|clip ?studio|clipstudiopaint|affinity|paint\.net|mspaint|paint|procreate|canva|davinci resolve|premiere|after effects|capcut|vegas|kdenlive|shotcut|obs|obs64|zbrush|maya|3ds max|cinema 4d|substance|sketchup|fusion 360|autocad|pixlr|photopea)\b"),
    ];
    raw.iter()
        .filter_map(|(k, p)| match Regex::new(&format!("(?i){p}")) {
            Ok(r) => Some((*k, r)),
            Err(e) => {
                log!("bad app pattern for {k}: {e}");
                None
            }
        })
        .collect()
});
static LINK_EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(lnk|url|exe|appref-ms)$").unwrap());

#[derive(Default)]
struct Reader {
    /// Shortcut path -> what it points at (target and arguments), so shortcuts are only opened once.
    targets: HashMap<String, String>,
    trash: i64,
    trash_at: Option<Instant>,
}

struct Icon {
    name: String,
    path: String,
    x: i32,
    y: i32,
}

struct Desk {
    icons: Vec<Icon>,
    space96: (f64, f64),
    icon_size: i32,
    snap: bool,
    auto_arrange: bool,
    hidden: bool,
}

impl Reader {
    fn read(&mut self) -> Result<Value, String> {
        let desk = unsafe { read_desk() }.map_err(|e| e.to_string())?;
        let monitors = desktop::monitors();
        let mut mons = Vec::new();
        for m in &monitors {
            let k = m.dpi as f64 / 96.0;
            let sx = (desk.space96.0 * k).round();
            let sy = (desk.space96.1 * k).round();
            let ww = m.work.right - m.work.left;
            let wh = m.work.bottom - m.work.top;
            mons.push(json!({
                "x": m.rect.left, "y": m.rect.top, "w": m.rect.right - m.rect.left, "h": m.rect.bottom - m.rect.top,
                "wx": m.work.left, "wy": m.work.top, "ww": ww, "wh": wh, "dpi": m.dpi, "p": m.primary,
                "sx": sx, "sy": sy, "ip": (desk.icon_size as f64 * k).round(),
                "cols": ((ww as f64) / sx.max(1.0)).floor().max(1.0),
                "rows": ((wh as f64) / sy.max(1.0)).floor().max(1.0),
            }));
        }
        let mut icons = Vec::new();
        for ic in &desk.icons {
            let mi = monitors
                .iter()
                .position(|m| {
                    ic.x + 8 >= m.rect.left && ic.x + 8 < m.rect.right && ic.y + 8 >= m.rect.top && ic.y + 8 < m.rect.bottom
                })
                .unwrap_or(0);
            let Some(m) = mons.get(mi) else { continue };
            let ip = m["ip"].as_f64().unwrap_or(48.0);
            let sx = m["sx"].as_f64().unwrap_or(1.0).max(1.0);
            let sy = m["sy"].as_f64().unwrap_or(1.0).max(1.0);
            let cx = ic.x as f64 + ip / 2.0;
            let cy = ic.y as f64 + ip / 2.0;
            let kind = self.kind(&ic.name, &ic.path);
            let named = kind != "file" && kind != "folder";
            icons.push(json!({
                "id": format!("{:08x}", util::fnv32(&ic.path.to_lowercase())),
                "k": kind,
                "n": if named { ic.name.as_str() } else { "" },
                "m": mi,
                "c": ((cx - m["wx"].as_f64().unwrap_or(0.0)) / sx).floor(),
                "r": ((cy - m["wy"].as_f64().unwrap_or(0.0)) / sy).floor(),
                "x": cx, "y": cy,
            }));
        }
        if self.trash_at.map(|t| t.elapsed() >= Duration::from_secs(5)).unwrap_or(true) {
            self.trash = trash_count();
            self.trash_at = Some(Instant::now());
        }
        Ok(json!({
            "v": 1, "on": true, "snap": desk.snap, "arrange": desk.auto_arrange, "hidden": desk.hidden,
            "monitors": mons, "icons": icons, "trash": self.trash,
        }))
    }

    fn kind(&mut self, name: &str, path: &str) -> &'static str {
        if path.to_uppercase().starts_with("::{645FF040-5081-101B-9F08-00AA002F954E}") {
            return "trash";
        }
        if path.starts_with("::{") {
            return "system";
        }
        if !LINK_EXT.is_match(path) {
            return if Path::new(path).is_dir() { "folder" } else { "file" };
        }
        let target = self.target(path);
        let hay = format!("{name} | {target}").to_lowercase();
        for (k, re) in KINDS.iter() {
            if re.is_match(&hay) {
                return k;
            }
        }
        "app"
    }

    fn target(&mut self, path: &str) -> String {
        if let Some(t) = self.targets.get(path) {
            return t.clone();
        }
        if self.targets.len() > 2000 {
            self.targets.clear();
        }
        let lower = path.to_lowercase();
        let t = if lower.ends_with(".lnk") {
            unsafe { shortcut_target(path) }.unwrap_or_default()
        } else if lower.ends_with(".url") {
            std::fs::read_to_string(path)
                .map(|s| {
                    s.lines()
                        .take(20)
                        .filter(|l| l.to_lowercase().starts_with("url="))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default()
        } else {
            path.to_string()
        };
        self.targets.insert(path.to_string(), t.clone());
        t
    }
}

unsafe fn shortcut_target(path: &str) -> Option<String> {
    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL).ok()?;
    link.cast::<IPersistFile>().ok()?.Load(&HSTRING::from(path), STGM_READ).ok()?;
    let mut target = [0u16; 1024];
    let _ = link.GetPath(&mut target, std::ptr::null_mut(), 0);
    let mut args = [0u16; 2048];
    let _ = link.GetArguments(&mut args);
    Some(format!("{} {}", util::from_wide(&target), util::from_wide(&args)))
}

fn trash_count() -> i64 {
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    match unsafe { SHQueryRecycleBinW(windows::core::PCWSTR::null(), &mut info) } {
        Ok(()) => info.i64NumItems,
        Err(_) => -1,
    }
}

/// The desktop's own folder view, as in Raymond Chen's "Manipulating the positions of desktop icons".
unsafe fn read_desk() -> windows::core::Result<Desk> {
    let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
    let loc = VARIANT::from(0i32); // CSIDL_DESKTOP
    let root = VARIANT::default();
    let mut hwnd = 0i32;
    let disp = windows.FindWindowSW(&loc, &root, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH)?;
    let sp: IServiceProvider = disp.cast()?;
    let browser: IShellBrowser = sp.QueryService(&SID_STopLevelBrowser)?;
    let view: IShellView = browser.QueryActiveShellView()?;
    let fv: IFolderView2 = view.cast()?;
    let folder: IShellFolder = fv.GetFolder()?;

    let sys = (GetDpiForSystem() as f64).max(96.0);
    let mut sp_pt = POINT::default();
    fv.GetSpacing(&mut sp_pt)?;
    let mut mode = Default::default();
    let mut size = 0i32;
    fv.GetViewModeAndIconSize(&mut mode, &mut size)?;
    let flags = fv.GetCurrentFolderFlags()?;
    // GetAutoArrange says yes with S_OK and no with S_FALSE, which the wrapper would treat the same.
    let hr = (Interface::vtable(&fv).base__.GetAutoArrange)(Interface::as_raw(&fv));
    let auto_arrange = hr.0 == 0;

    let def_view: HWND = view.GetWindow()?;
    let list = FindWindowExW(Some(def_view), None, windows::core::w!("SysListView32"), windows::core::PCWSTR::null()).unwrap_or_default();
    let mut org = POINT::default();
    let _ = ClientToScreen(if list.is_invalid() { def_view } else { list }, &mut org);
    // "Show desktop icons" off: Windows still lists the icons and their places, it just doesn't draw them.
    let hidden = !list.is_invalid() && !IsWindowVisible(list).as_bool();

    let n = fv.ItemCount(SVGIO_ALLVIEW)?;
    let mut icons = Vec::with_capacity(n.max(0) as usize);
    for i in 0..n {
        let Ok(pidl) = fv.Item(i) else { continue };
        let pos = fv.GetItemPosition(pidl);
        let name = display_name(&folder, pidl, 0);
        let path = display_name(&folder, pidl, 0x8000); // SHGDN_FORPARSING
        CoTaskMemFree(Some(pidl as *const _));
        if let Ok(p) = pos {
            icons.push(Icon { name, path, x: p.x + org.x, y: p.y + org.y });
        }
    }
    Ok(Desk {
        icons,
        space96: (sp_pt.x as f64 * 96.0 / sys, sp_pt.y as f64 * 96.0 / sys),
        icon_size: size,
        snap: flags & 0x4 != 0, // FWF_SNAPTOGRID
        auto_arrange,
        hidden,
    })
}

unsafe fn display_name(folder: &IShellFolder, pidl: *mut ITEMIDLIST, flags: u32) -> String {
    let mut sr = STRRET::default();
    if folder
        .GetDisplayNameOf(pidl, windows::Win32::UI::Shell::SHGDNF(flags), &mut sr)
        .is_err()
    {
        return String::new();
    }
    let mut out = PWSTR::null();
    if StrRetToStrW(&mut sr, Some(pidl), &mut out).is_err() {
        return String::new();
    }
    let s = out.to_string().unwrap_or_default();
    CoTaskMemFree(Some(out.0 as *const _));
    s
}
