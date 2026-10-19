// The app itself: which wallpaper plays where, when to pause, and what the window and tray ask for.
// Everything here runs on the UI thread, one Event at a time (see host.rs).

use std::{collections::HashMap, rc::Rc, cell::RefCell, time::{Duration, Instant}};

use serde_json::{json, Map, Value};
use webview2_com::{
    CoreWebView2EnvironmentOptions, CreateCoreWebView2EnvironmentCompletedHandler,
    Microsoft::Web::WebView2::Win32::{CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2Environment, ICoreWebView2EnvironmentOptions},
};
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::{HWND, RECT},
        System::{
            Power::{GetSystemPowerStatus, RegisterPowerSettingNotification, SYSTEM_POWER_STATUS},
            RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION},
            SystemServices::GUID_CONSOLE_DISPLAY_STATE,
        },
        UI::{
            Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
            WindowsAndMessaging::*,
        },
    },
};

use crate::{
    audio::Capture,
    config::Config,
    desktop::{self, Monitor},
    dialogs, ears,
    host::{self, timer, Deferred, Event, FilePurpose, WorkerEvent},
    input,
    library::{self, Helper, Library, Source},
    log,
    manager::{self, Folders, Manager},
    surface::Surface,
    tray, util,
};

/// Flags for the browser engine shared by every wallpaper and the window.
const BROWSER_ARGS: &str = "--autoplay-policy=no-user-gesture-required \
    --disable-features=CalculateNativeWinOcclusion,msSmartScreenProtection";

#[derive(PartialEq)]
enum EnvState {
    Pending,
    Ready,
    Failed,
}

pub struct App {
    host: HWND,
    cfg: Config,
    lib: Library,
    folders: Folders,
    env: Option<ICoreWebView2Environment>,
    env_state: EnvState,
    layer: Option<HWND>,
    monitors: Vec<Monitor>,
    surfaces: Vec<Surface>,
    manager: Option<Manager>,
    open_manager_when_ready: bool,
    next_uid: u64,
    covered: Vec<bool>,
    locked: bool,
    screen_on: bool,
    on_battery: bool,
    audio: Option<Capture>,
    input_key: Vec<(u64, RECT)>,
    polls: u64,
    peeking: bool,
    /// Explorer restarted and a rebuild is scheduled; don't rebuild early while it starts up.
    rebuild_pending: bool,
    /// When the browser engine had to be restarted, to give up if it keeps dying.
    restarts: Vec<Instant>,
    /// Wallpapers whose browser failed to start recently, to retry a few times and then stop.
    start_failures: u32,
    hooks: Vec<HWINEVENTHOOK>,
    quitting: bool,
}

impl App {
    pub fn new(host: HWND, show_window: bool) -> App {
        let first_run = !util::data_dir().join("config.json").exists();
        let mut cfg = Config::load();
        if util::test_data_dir().is_some() {
            // A test run: leave the real Start-with-Windows entry alone.
        } else if first_run {
            // A wallpaper app that doesn't come back after a restart isn't much use, so this starts on.
            // It's one click to turn off, in the tray menu and the app's settings.
            dialogs::set_autostart(cfg.autostart);
            cfg.save();
        } else {
            dialogs::refresh_autostart();
        }
        cfg.autostart = dialogs::autostart_enabled();
        let lib = Library::scan();
        let folders: Folders = Rc::new(RefCell::new(HashMap::new()));
        let mut app = App {
            host,
            cfg,
            lib,
            folders,
            env: None,
            env_state: EnvState::Pending,
            layer: None,
            monitors: Vec::new(),
            surfaces: Vec::new(),
            manager: None,
            open_manager_when_ready: show_window || first_run,
            next_uid: 1,
            covered: Vec::new(),
            locked: false,
            screen_on: true,
            on_battery: false,
            audio: None,
            input_key: Vec::new(),
            polls: 0,
            peeking: false,
            rebuild_pending: false,
            restarts: Vec::new(),
            start_failures: 0,
            hooks: Vec::new(),
            quitting: false,
        };
        app.refresh_folders();
        app
    }

    pub fn start(&mut self) {
        tray::add(self.host, util::APP_NAME);
        input::register(self.host);
        unsafe {
            let _ = WTSRegisterSessionNotification(self.host, NOTIFY_FOR_THIS_SESSION);
            let _ = RegisterPowerSettingNotification(
                windows::Win32::Foundation::HANDLE(self.host.0),
                &GUID_CONSOLE_DISPLAY_STATE,
                windows::Win32::UI::WindowsAndMessaging::REGISTER_NOTIFICATION_FLAGS(0),
            );
            // Windows coming to the front, minimising or being moved: check the desktop right away.
            for (a, b) in [
                (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
                (EVENT_SYSTEM_MOVESIZEEND, EVENT_SYSTEM_MOVESIZEEND),
                (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
            ] {
                let h = SetWinEventHook(a, b, None, Some(host::win_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
                if !h.is_invalid() {
                    self.hooks.push(h);
                }
            }
            SetTimer(Some(self.host), timer::POLL, 500, None);
            SetTimer(Some(self.host), timer::HELPERS, 5000, None);
        }
        self.on_battery = on_battery();
        self.create_environment();
    }

    fn create_environment(&mut self) {
        let options = CoreWebView2EnvironmentOptions::default();
        let mut args = BROWSER_ARGS.to_string();
        // Debug builds only: LIVEWALLPAPER_DEBUG_PORT=9229 opens the pages to Chrome DevTools for testing.
        if cfg!(debug_assertions) {
            if let Ok(port) = std::env::var("LIVEWALLPAPER_DEBUG_PORT") {
                if port.parse::<u16>().is_ok() {
                    args.push_str(&format!(" --remote-debugging-port={port}"));
                }
            }
        }
        unsafe { options.set_additional_browser_arguments(args) };
        let options: ICoreWebView2EnvironmentOptions = options.into();
        let folder = HSTRING::from(util::data_dir().join("WebView2").as_os_str());
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(|hr, env| {
            let result = hr.and_then(|_| env.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER)));
            host::post(Event::EnvironmentReady(result));
            Ok(())
        }));
        if let Err(e) = unsafe { CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), &folder, &options, &handler) } {
            host::post(Event::EnvironmentReady(Err(e)));
        }
    }

    pub fn handle(&mut self, ev: Event) -> Option<Deferred> {
        if self.quitting {
            return None;
        }
        match ev {
            Event::EnvironmentReady(Ok(env)) => {
                self.env = Some(env);
                self.env_state = EnvState::Ready;
                self.rebuild();
                if self.open_manager_when_ready {
                    self.open_manager_when_ready = false;
                    self.open_manager();
                }
            }
            Event::EnvironmentReady(Err(e)) => {
                log!("WebView2 couldn't start: {e}");
                self.env_state = EnvState::Failed;
                return Some(Box::new(|| {
                    let r = unsafe {
                        MessageBoxW(
                            None,
                            &HSTRING::from(
                                "Live Wallpaper needs the Microsoft Edge WebView2 Runtime, which comes with Windows 11 and \
                                 most Windows 10 PCs, but isn't on this one (or didn't start).\n\n\
                                 Open the download page now? Install the \"Evergreen Bootstrapper\", then start Live Wallpaper again.",
                            ),
                            &HSTRING::from(util::APP_NAME),
                            MB_YESNO | MB_ICONWARNING,
                        )
                    };
                    if r == IDYES {
                        dialogs::open("https://developer.microsoft.com/microsoft-edge/webview2/");
                    }
                    host::post(Event::TrayCommand(tray::CMD_QUIT));
                }));
            }
            Event::SurfaceController { uid, result } => self.surface_ready(uid, result),
            Event::SurfaceNavigated { uid, ok } => self.surface_loaded(uid, ok),
            Event::SurfaceCrashed { uid, why } => self.surface_crashed(uid, &why),
            Event::BrowserGone => {
                // Every surface reports this; the first one restarts the engine and the rest find nothing to do.
                if self.env_state != EnvState::Ready {
                    return None;
                }
                log!("the browser engine stopped; starting it again");
                for s in self.surfaces.drain(..) {
                    s.destroy();
                }
                let reopen = self.manager.is_some();
                if let Some(m) = self.manager.take() {
                    m.close();
                }
                self.env = None;
                self.env_state = EnvState::Pending;
                self.open_manager_when_ready = reopen;
                self.input_key.clear();
                self.update_pause();
                self.restarts.retain(|t| t.elapsed() < Duration::from_secs(300));
                self.restarts.push(Instant::now());
                if self.restarts.len() > 3 {
                    log!("the browser engine keeps stopping; leaving the wallpapers off until the app is restarted");
                    self.env_state = EnvState::Failed;
                    desktop::restore_windows_wallpaper();
                } else {
                    self.create_environment();
                }
            }
            Event::ManagerController(result) => {
                let (Some(env), Some(m)) = (self.env.clone(), self.manager.as_mut()) else { return None };
                match result {
                    Ok(c) => {
                        if let Err(e) = m.attach(&env, c, self.folders.clone(), self.cfg.developer) {
                            log!("the window's page couldn't start: {e}");
                        }
                    }
                    Err(e) => {
                        log!("the window's browser couldn't start: {e}");
                        if let Some(m) = self.manager.take() {
                            m.close();
                        }
                    }
                }
            }
            Event::ManagerNavigated => {
                if let Some(m) = self.manager.as_mut() {
                    m.loaded();
                }
            }
            Event::ManagerMessage { json, files } => return self.ui_message(&json, files),
            Event::ManagerCloseRequested => {
                if let Some(m) = self.manager.take() {
                    m.close();
                }
            }
            Event::Tray { msg } => {
                const NIN_SELECT: u32 = WM_USER;
                const NIN_KEYSELECT: u32 = WM_USER + 1;
                match msg {
                    NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONDBLCLK => self.open_manager(),
                    WM_CONTEXTMENU | WM_RBUTTONUP => {
                        let (host, paused, auto) = (self.host, self.cfg.paused, self.cfg.autostart);
                        return Some(Box::new(move || {
                            let cmd = tray::menu(host, paused, auto);
                            if cmd != 0 {
                                host::post(Event::TrayCommand(cmd));
                            }
                        }));
                    }
                    _ => {}
                }
            }
            Event::TrayCommand(cmd) => match cmd {
                tray::CMD_OPEN => self.open_manager(),
                tray::CMD_PAUSE => self.set_setting("paused", &json!(!self.cfg.paused)),
                tray::CMD_AUTOSTART => self.set_setting("autostart", &json!(!self.cfg.autostart)),
                tray::CMD_QUIT => self.quit(),
                _ => {}
            },
            Event::Timer(id) => return self.on_timer(id),
            Event::DisplaysChanged => unsafe {
                SetTimer(Some(self.host), timer::DISPLAYS, 700, None);
            },
            Event::ExplorerRestarted => {
                log!("Explorer restarted; putting the wallpapers back");
                for s in self.surfaces.drain(..) {
                    s.destroy();
                }
                self.layer = None;
                self.rebuild_pending = true;
                unsafe {
                    SetTimer(Some(self.host), timer::REBUILD, 2500, None);
                }
            }
            Event::SessionLocked(l) => {
                self.locked = l;
                self.update_pause();
            }
            Event::ScreenPower(on) => {
                self.screen_on = on;
                self.update_pause();
            }
            Event::PowerChanged => {
                self.on_battery = on_battery();
                self.update_pause();
            }
            Event::ThemeChanged => {
                if let Some(m) = &self.manager {
                    manager::apply_theme(m.hwnd);
                }
            }
            Event::ShowManager => self.open_manager(),
            Event::FilesChosen { purpose, paths } => self.files_chosen(purpose, paths),
            Event::PageScript { window, js } => {
                if let Some(s) = self.surfaces.iter().find(|s| s.hwnd == window && s.loaded && !s.paused) {
                    s.exec(&js);
                }
            }
            Event::Worker(WorkerEvent::EarsReply { token, body }) => ears::finish(token, &body),
            Event::Worker(WorkerEvent::ImportDone { results, target }) => self.import_done(results, &target),
            Event::EndSession => self.cfg.save(),
        }
        None
    }

    // ---------- the wallpapers on screen ----------

    /// Make what's on screen match the settings: one surface per screen that has a wallpaper.
    fn rebuild(&mut self) {
        let Some(env) = self.env.clone() else { return };
        self.monitors = desktop::monitors();
        let layer = desktop::wallpaper_layer();
        if layer != self.layer {
            for s in self.surfaces.drain(..) {
                s.destroy();
            }
        }
        self.layer = layer;
        let Some(layer) = layer else {
            log!("couldn't find the desktop (is Explorer running?)");
            return;
        };
        let wanted: Vec<(usize, Option<String>)> = self
            .monitors
            .iter()
            .enumerate()
            .map(|(i, m)| (i, self.cfg.wallpaper_for(&m.id).filter(|id| self.lib.get(id).is_some())))
            .collect();
        let mut keep: Vec<Surface> = Vec::new();
        let mut emptied = false;
        for s in self.surfaces.drain(..) {
            let fits = self.monitors.iter().enumerate().find(|(_, m)| m.id == s.display).and_then(|(i, m)| {
                (wanted[i].1.as_deref() == Some(s.wallpaper.as_str()) && m.rect == s.rect && s.alive(layer)).then_some(i)
            });
            match fits {
                Some(i) if !keep.iter().any(|k| k.display == s.display) => {
                    s.screen_index.set(i as i32);
                    keep.push(s);
                }
                _ => {
                    emptied = true;
                    s.destroy();
                }
            }
        }
        for (i, id) in wanted {
            let Some(id) = id else { continue };
            let m = &self.monitors[i];
            if keep.iter().any(|s| s.display == m.id) {
                continue;
            }
            let w = self.lib.get(&id).unwrap();
            let uid = self.next_uid;
            self.next_uid += 1;
            match Surface::create(&env, layer, m, i as i32, w, uid) {
                Ok(s) => keep.push(s),
                Err(e) => log!("couldn't put {} on {}: {e}", w.title, m.name),
            }
        }
        self.surfaces = keep;
        if emptied {
            // Somewhere a wallpaper was taken away: have Explorer repaint the normal one there.
            let all_covered = self.monitors.iter().all(|m| self.surfaces.iter().any(|s| s.display == m.id));
            if !all_covered {
                desktop::restore_windows_wallpaper();
            }
        }
        self.covered = desktop::covered(&self.monitors);
        self.update_pause();
        self.send_state();
    }

    fn surface_ready(&mut self, uid: u64, result: windows::core::Result<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller>) {
        let Some(env) = self.env.clone() else { return };
        let Some(i) = self.surfaces.iter().position(|s| s.uid == uid) else {
            // It was replaced before its browser arrived.
            if let Ok(c) = result {
                unsafe {
                    let _ = c.Close();
                }
            }
            return;
        };
        let wid = self.surfaces[i].wallpaper.clone();
        let res = match (result, self.lib.get(&wid)) {
            (Ok(c), Some(w)) => self.surfaces[i].attach(&env, c, w, self.cfg.developer),
            (Err(e), _) => Err(e),
            (Ok(c), None) => {
                unsafe {
                    let _ = c.Close();
                }
                Ok(())
            }
        };
        match res {
            Ok(()) => self.start_failures = 0,
            Err(e) => {
                log!("a wallpaper's browser couldn't start: {e}");
                let s = self.surfaces.remove(i);
                s.destroy();
                // Try again in a few seconds (a rebuild fills the gap), but not forever.
                self.start_failures += 1;
                if self.start_failures <= 3 {
                    unsafe {
                        SetTimer(Some(self.host), timer::DISPLAYS, 3000, None);
                    }
                }
            }
        }
    }

    fn surface_loaded(&mut self, uid: u64, ok: bool) {
        let Some(s) = self.surfaces.iter_mut().find(|s| s.uid == uid) else { return };
        if !ok {
            log!("{} didn't load", s.wallpaper);
        }
        s.loaded = true;
        if let Some(w) = self.lib.get(&s.wallpaper) {
            s.send_properties(&library::startup_values(w, self.cfg.properties.get(&w.id)));
        }
        if s.paused {
            // It was paused before the page could hear it.
            s.paused = false;
            s.set_paused(true);
        }
        self.input_key.clear();
        self.update_pause();
    }

    fn surface_crashed(&mut self, uid: u64, why: &str) {
        let Some(i) = self.surfaces.iter().position(|s| s.uid == uid) else { return };
        let s = &mut self.surfaces[i];
        log!("{} stopped ({why})", s.wallpaper);
        s.crashes.retain(|t| t.elapsed() < Duration::from_secs(60));
        s.crashes.push(Instant::now());
        if s.crashes.len() > 3 {
            log!("{} keeps stopping, so it's left off until you pick it again", s.wallpaper);
            let s = self.surfaces.remove(i);
            s.destroy();
            desktop::restore_windows_wallpaper();
            return;
        }
        // Start it afresh: the rebuild sees the gap and fills it.
        let (display, crashes) = (s.display.clone(), s.crashes.clone());
        let s = self.surfaces.remove(i);
        s.destroy();
        self.rebuild();
        if let Some(n) = self.surfaces.iter_mut().find(|n| n.display == display) {
            n.crashes = crashes;
        }
    }

    fn monitor_index(&self, display: &str) -> Option<usize> {
        self.monitors.iter().position(|m| m.id == display)
    }

    /// Pause what nobody can see; freeze it (and free memory) if that lasts a few seconds.
    fn update_pause(&mut self) {
        let now = Instant::now();
        for i in 0..self.surfaces.len() {
            let idx = self.monitor_index(&self.surfaces[i].display);
            let covered = idx.and_then(|i| self.covered.get(i).copied()).unwrap_or(false);
            let unseen = self.locked || !self.screen_on;
            let hide = unseen || (self.cfg.pause_when_covered && covered);
            let soft = hide || self.cfg.paused || (self.cfg.pause_on_battery && self.on_battery);
            let s = &mut self.surfaces[i];
            s.set_paused(soft);
            if hide {
                let since = *s.hidden_since.get_or_insert(now);
                if unseen || now.duration_since(since) >= Duration::from_secs(4) {
                    s.suspend();
                }
            } else {
                s.hidden_since = None;
                s.resume();
            }
        }
        self.update_input();
        self.update_audio();
    }

    fn update_input(&mut self) {
        let live: Vec<&Surface> = if self.cfg.mouse_input {
            self.surfaces.iter().filter(|s| s.loaded && !s.paused).collect()
        } else {
            Vec::new()
        };
        let key: Vec<(u64, RECT)> = live.iter().map(|s| (s.uid, s.rect)).collect();
        if key == self.input_key {
            return;
        }
        input::set_targets(live.iter().map(|s| input::Target { rect: s.rect, window: s.hwnd }).collect());
        self.input_key = key;
    }

    fn update_audio(&mut self) {
        let want = self.surfaces.iter().any(|s| s.audio && s.loaded && !s.paused);
        if want && self.audio.is_none() {
            self.audio = Some(Capture::start());
            unsafe {
                SetTimer(Some(self.host), timer::AUDIO, 30, None);
            }
        } else if !want && self.audio.is_some() {
            self.audio = None;
            unsafe {
                let _ = KillTimer(Some(self.host), timer::AUDIO);
            }
        }
    }

    fn on_timer(&mut self, id: usize) -> Option<Deferred> {
        let one_shot = matches!(id, timer::SAVE | timer::DISPLAYS | timer::REBUILD | timer::QUICK);
        if one_shot {
            unsafe {
                let _ = KillTimer(Some(self.host), id);
            }
        }
        match id {
            timer::POLL => {
                self.polls += 1;
                self.on_battery = on_battery();
                self.covered = desktop::covered(&self.monitors);
                // Now and then, make sure the wallpapers are still where they should be.
                if self.polls % 4 == 0 && self.env.is_some() && !self.rebuild_pending {
                    let lost = match self.layer {
                        Some(l) => !unsafe { IsWindow(Some(l)).as_bool() } || self.surfaces.iter().any(|s| !s.alive(l)),
                        None => self.surfaces.is_empty() && self.monitors.iter().any(|m| self.cfg.wallpaper_for(&m.id).is_some()),
                    };
                    if lost {
                        log!("the desktop changed under the wallpapers; putting them back");
                        self.rebuild();
                        return None;
                    }
                }
                self.update_pause();
            }
            timer::QUICK => {
                self.covered = desktop::covered(&self.monitors);
                self.update_pause();
            }
            timer::SAVE => self.cfg.save(),
            timer::DISPLAYS => self.rebuild(),
            timer::REBUILD => {
                self.rebuild_pending = false;
                tray::add(self.host, util::APP_NAME);
                self.rebuild();
            }
            timer::AUDIO => {
                if let Some(js) = self.audio.as_ref().and_then(|a| a.levels_js()) {
                    let call = format!("window.livelyAudioListener&&window.livelyAudioListener({js});");
                    for s in self.surfaces.iter().filter(|s| s.audio && s.loaded && !s.paused) {
                        s.exec(&call);
                    }
                }
            }
            timer::HELPERS => ears::expire_stale(),
            timer::PEEK => {
                let held = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000 != 0;
                if !held {
                    unsafe {
                        let _ = KillTimer(Some(self.host), timer::PEEK);
                    }
                    self.peeking = false;
                    if let Some(m) = &self.manager {
                        m.focus();
                    }
                }
            }
            _ => {}
        }
        None
    }

    fn save_soon(&self) {
        unsafe {
            SetTimer(Some(self.host), timer::SAVE, 600, None);
        }
    }

    fn open_manager(&mut self) {
        match self.env_state {
            EnvState::Pending => {
                self.open_manager_when_ready = true;
                return;
            }
            EnvState::Failed => return,
            EnvState::Ready => {}
        }
        if let Some(m) = &self.manager {
            m.focus();
            return;
        }
        let Some(env) = self.env.clone() else { return };
        match Manager::open(&env) {
            Ok(m) => self.manager = Some(m),
            Err(e) => log!("couldn't open the window: {e}"),
        }
    }

    fn quit(&mut self) {
        if let Some(m) = self.manager.take() {
            m.close();
        }
        let had = !self.surfaces.is_empty();
        for s in self.surfaces.drain(..) {
            s.destroy();
        }
        if had {
            desktop::restore_windows_wallpaper();
        }
        self.audio = None;
        input::set_targets(Vec::new());
        tray::remove(self.host);
        for h in self.hooks.drain(..) {
            unsafe {
                let _ = UnhookWinEvent(h);
            }
        }
        self.cfg.save();
        self.quitting = true;
        unsafe { PostQuitMessage(0) };
    }

    // ---------- the window ----------

    fn refresh_folders(&mut self) {
        let mut f = self.folders.borrow_mut();
        f.clear();
        for w in &self.lib.items {
            f.insert(w.id.clone(), w.dir.clone());
        }
    }

    fn reload_library(&mut self) {
        self.lib = Library::scan();
        self.refresh_folders();
    }

    fn post_ui(&mut self, v: &Value) {
        if let Some(m) = self.manager.as_mut() {
            m.post(&v.to_string());
        }
    }

    fn toast(&mut self, text: &str, kind: &str) {
        self.post_ui(&json!({ "type": "toast", "text": text, "kind": kind }));
    }

    fn send_state(&mut self) {
        if self.manager.is_none() {
            return;
        }
        let v = self.state_json();
        self.post_ui(&v);
    }

    fn state_json(&self) -> Value {
        let wallpapers: Vec<Value> = self
            .lib
            .items
            .iter()
            .map(|w| {
                let settings = w
                    .props
                    .iter()
                    .filter(|(_, d)| d.get("type").and_then(|t| t.as_str()) != Some("label"))
                    .count();
                json!({
                    "id": w.id, "title": w.title, "desc": w.desc, "author": w.author, "builtin": w.builtin,
                    "kind": w.kind,
                    "thumb": w.thumbnail.as_ref().map(|t| manager::file_url(&w.id, t)),
                    "preview": w.preview.as_ref().map(|t| manager::file_url(&w.id, t)),
                    "settings": settings,
                    "usesMouse": w.uses_mouse,
                    "helper": match w.helper { Helper::Eyes => json!("eyes"), Helper::Ears => json!("ears"), Helper::None => Value::Null },
                    "audio": w.audio,
                    "website": matches!(w.source, Source::Url(_)),
                })
            })
            .collect();
        let displays: Vec<Value> = self
            .monitors
            .iter()
            .map(|m| {
                json!({
                    "id": m.id, "name": m.name,
                    "width": m.rect.right - m.rect.left, "height": m.rect.bottom - m.rect.top,
                    "primary": m.primary,
                    "wallpaper": self.cfg.wallpaper_for(&m.id).filter(|id| self.lib.get(id).is_some()),
                })
            })
            .collect();
        json!({
            "type": "state",
            "version": util::VERSION,
            "wallpapers": wallpapers,
            "displays": displays,
            "welcomed": self.cfg.welcomed,
            "settings": {
                "paused": self.cfg.paused,
                "pauseWhenCovered": self.cfg.pause_when_covered,
                "pauseOnBattery": self.cfg.pause_on_battery,
                "mouseInput": self.cfg.mouse_input,
                "developer": self.cfg.developer,
                "autostart": self.cfg.autostart,
            },
        })
    }

    fn props_json(&self, id: &str) -> Value {
        let Some(w) = self.lib.get(id) else { return json!({ "type": "props", "id": id, "items": [] }) };
        let user = self.cfg.properties.get(id);
        let mut items = Vec::new();
        let mut values = Map::new();
        let mut folders = Map::new();
        for (k, def) in &w.props {
            let mut d = def.clone();
            d["key"] = json!(k);
            items.push(d);
            let v = user
                .and_then(|m| m.get(k))
                .and_then(|v| library::coerce(def, v))
                .or_else(|| def.get("value").cloned())
                .unwrap_or(Value::Null);
            values.insert(k.clone(), v);
            if def.get("type").and_then(|t| t.as_str()) == Some("folderDropdown") {
                let folder = def.get("folder").and_then(|f| f.as_str()).unwrap_or("");
                let files: Vec<Value> = library::folder_files(w, def)
                    .into_iter()
                    .map(|f| {
                        let rel = if folder.is_empty() { f.clone() } else { format!("{folder}/{f}") };
                        json!({ "name": f, "url": manager::file_url(id, &rel) })
                    })
                    .collect();
                folders.insert(k.clone(), Value::Array(files));
            }
        }
        json!({ "type": "props", "id": id, "items": items, "values": values, "folders": folders })
    }

    fn surfaces_of(&self, id: &str) -> impl Iterator<Item = &Surface> {
        let id = id.to_string();
        self.surfaces.iter().filter(move |s| s.wallpaper == id)
    }

    fn set_property(&mut self, id: &str, key: &str, value: &Value) {
        let Some(w) = self.lib.get(id) else { return };
        let Some(def) = w.prop(key).cloned() else { return };
        let Some(v) = library::coerce(&def, value) else { return };
        let entry = self.cfg.properties.entry(id.to_string()).or_default();
        if def.get("value") == Some(&v) {
            entry.remove(key);
        } else {
            entry.insert(key.to_string(), v.clone());
        }
        if entry.is_empty() {
            self.cfg.properties.remove(id);
        }
        self.save_soon();
        let send = vec![(key.to_string(), library::dispatch_value(&def, &v))];
        for s in self.surfaces_of(id) {
            if s.loaded {
                s.send_properties(&send);
            }
        }
    }

    /// Put a wallpaper on a screen ("all" for every screen), or take it off (None).
    fn apply(&mut self, target: &str, id: Option<String>) {
        if target == "all" {
            self.cfg.default_wallpaper = id.clone();
            for m in &self.monitors {
                self.cfg.displays.insert(m.id.clone(), id.clone());
            }
        } else {
            self.cfg.displays.insert(target.to_string(), id.clone());
            if self.cfg.default_wallpaper.is_none() && id.is_some() {
                self.cfg.default_wallpaper = id;
            }
        }
        self.cfg.welcomed = true;
        self.save_soon();
        self.rebuild();
    }

    fn set_setting(&mut self, key: &str, v: &Value) {
        let b = v.as_bool().unwrap_or(false);
        match key {
            "paused" => {
                self.cfg.paused = b;
                tray::set_tooltip(self.host, if b { "Live Wallpaper (paused)" } else { util::APP_NAME });
            }
            "pauseWhenCovered" => self.cfg.pause_when_covered = b,
            "pauseOnBattery" => self.cfg.pause_on_battery = b,
            "mouseInput" => {
                self.cfg.mouse_input = b;
                self.input_key.clear();
            }
            "developer" => {
                self.cfg.developer = b;
                for s in &self.surfaces {
                    s.set_developer(b);
                }
                if let Some(m) = &self.manager {
                    m.set_developer(b);
                }
            }
            "autostart" => {
                dialogs::set_autostart(b);
                self.cfg.autostart = dialogs::autostart_enabled();
            }
            "welcomed" => self.cfg.welcomed = b,
            _ => return,
        }
        self.save_soon();
        self.update_pause();
        self.send_state();
    }

    fn ui_message(&mut self, json: &str, files: Vec<String>) -> Option<Deferred> {
        let Ok(msg) = serde_json::from_str::<Value>(json) else { return None };
        let s = |k: &str| msg.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        match s("type").as_str() {
            "hello" => self.send_state(),
            "getProps" => {
                let v = self.props_json(&s("id"));
                self.post_ui(&v);
            }
            "setProp" => self.set_property(&s("id"), &s("key"), msg.get("value").unwrap_or(&Value::Null)),
            "button" => {
                let id = s("id");
                let key = s("key");
                let Some(w) = self.lib.get(&id) else { return None };
                // A button can list settings it puts back to their defaults ("resets": [...]). Lively ignores this.
                let resets: Vec<String> = w
                    .prop(&key)
                    .and_then(|d| d.get("resets"))
                    .and_then(|r| r.as_array())
                    .map(|a| a.iter().filter_map(|k| k.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let mut send: Vec<(String, Value)> = Vec::new();
                for k in &resets {
                    let Some(def) = w.prop(k) else { continue };
                    if let Some(m) = self.cfg.properties.get_mut(&id) {
                        m.remove(k);
                    }
                    if let Some(v) = def.get("value") {
                        send.push((k.clone(), library::dispatch_value(def, v)));
                    }
                }
                if self.cfg.properties.get(&id).map(|m| m.is_empty()).unwrap_or(false) {
                    self.cfg.properties.remove(&id);
                }
                send.push((key, json!(true)));
                for sf in self.surfaces_of(&id) {
                    if sf.loaded {
                        sf.send_properties(&send);
                    }
                }
                if !resets.is_empty() {
                    self.save_soon();
                    let v = self.props_json(&id);
                    self.post_ui(&v);
                }
            }
            "resetProps" => {
                let id = s("id");
                self.cfg.properties.remove(&id);
                self.save_soon();
                if let Some(w) = self.lib.get(&id) {
                    let values = library::startup_values(w, None);
                    for sf in self.surfaces_of(&id) {
                        if sf.loaded {
                            sf.send_properties(&values);
                        }
                    }
                }
                let v = self.props_json(&id);
                self.post_ui(&v);
            }
            "apply" => {
                let id = s("id");
                if self.lib.get(&id).is_some() {
                    self.apply(&s("target"), Some(id));
                }
            }
            "off" => self.apply(&s("target"), None),
            "import" => {
                let target = s("target");
                let folder = msg.get("folder").and_then(|v| v.as_bool()).unwrap_or(false);
                let owner = self.manager.as_ref().map(|m| m.hwnd);
                return Some(Box::new(move || {
                    let paths = if folder {
                        dialogs::pick(owner, "Add a wallpaper folder", &[], false, true)
                    } else {
                        dialogs::pick(
                            owner,
                            "Add wallpapers",
                            &[
                                ("Wallpapers, pictures and videos", "*.zip;*.html;*.htm;*.jpg;*.jpeg;*.png;*.gif;*.webp;*.bmp;*.avif;*.mp4;*.webm;*.mov;*.m4v"),
                                ("Lively wallpaper (.zip)", "*.zip"),
                                ("Web page", "*.html;*.htm"),
                                ("Pictures", "*.jpg;*.jpeg;*.png;*.gif;*.webp;*.bmp;*.avif"),
                                ("Videos", "*.mp4;*.webm;*.mov;*.m4v"),
                            ],
                            true,
                            false,
                        )
                    };
                    if !paths.is_empty() {
                        host::post(Event::FilesChosen { purpose: FilePurpose::Import { target }, paths });
                    }
                }));
            }
            "drop" => {
                if !files.is_empty() {
                    self.files_chosen(FilePurpose::Import { target: s("target") }, files);
                }
            }
            "addPicture" => {
                let (id, key) = (s("id"), s("key"));
                let owner = self.manager.as_ref().map(|m| m.hwnd);
                return Some(Box::new(move || {
                    let paths = dialogs::pick(
                        owner,
                        "Add a picture",
                        &[("Pictures", "*.jpg;*.jpeg;*.png;*.webp;*.bmp;*.gif;*.avif")],
                        false,
                        false,
                    );
                    if !paths.is_empty() {
                        host::post(Event::FilesChosen { purpose: FilePurpose::Picture { id, key }, paths });
                    }
                }));
            }
            "openFolder" => {
                let id = s("id");
                let dir = match id.as_str() {
                    "" | "user" => library::user_dir(),
                    "data" => util::data_dir(),
                    _ => match self.lib.get(&id) {
                        Some(w) => w.dir.clone(),
                        None => return None,
                    },
                };
                dialogs::open(&dir.display().to_string());
            }
            "delete" => {
                let id = s("id");
                let Some(w) = self.lib.get(&id).cloned() else { return None };
                match library::delete(&w) {
                    Ok(()) => {
                        for v in self.cfg.displays.values_mut() {
                            if v.as_deref() == Some(id.as_str()) {
                                *v = None;
                            }
                        }
                        if self.cfg.default_wallpaper.as_deref() == Some(id.as_str()) {
                            self.cfg.default_wallpaper = None;
                        }
                        self.cfg.properties.remove(&id);
                        self.save_soon();
                        self.reload_library();
                        self.rebuild();
                        self.toast(&format!("Removed {}.", w.title), "ok");
                    }
                    Err(e) => self.toast(&e, "error"),
                }
            }
            "reload" => {
                let id = s("id");
                self.reload_library();
                for sf in self.surfaces.iter_mut().filter(|sf| sf.wallpaper == id) {
                    sf.reload();
                }
                self.send_state();
                let v = self.props_json(&id);
                self.post_ui(&v);
            }
            "devtools" => {
                let id = s("id");
                let opened = match self.surfaces_of(&id).next() {
                    Some(sf) => {
                        sf.open_devtools();
                        true
                    }
                    None => false,
                };
                if !opened {
                    self.toast("Put it on a screen first, then open its developer tools.", "info");
                }
            }
            "newWallpaper" => match library::create_starter() {
                Ok(id) => {
                    self.reload_library();
                    if let Some(w) = self.lib.get(&id) {
                        dialogs::open(&w.dir.display().to_string());
                    }
                    self.apply(&s("target"), Some(id.clone()));
                    self.post_ui(&json!({ "type": "imported", "id": id }));
                    self.toast("Made a starter wallpaper. Edit its index.html, then press Reload.", "ok");
                }
                Err(e) => self.toast(&e, "error"),
            },
            "setSetting" => self.set_setting(&s("key"), msg.get("value").unwrap_or(&Value::Null)),
            "peek" => {
                if let Some(m) = &self.manager {
                    self.peeking = true;
                    unsafe {
                        let _ = ShowWindow(m.hwnd, SW_HIDE);
                        SetTimer(Some(self.host), timer::PEEK, 50, None);
                    }
                }
            }
            "openLink" => {
                let u = s("url");
                if u.starts_with("https://") {
                    dialogs::open(&u);
                }
            }
            "quit" => self.quit(),
            _ => {}
        }
        None
    }

    fn files_chosen(&mut self, purpose: FilePurpose, paths: Vec<String>) {
        match purpose {
            FilePurpose::Import { target } => {
                self.post_ui(&json!({ "type": "busy", "on": true, "text": "Adding…" }));
                let paths: Vec<std::path::PathBuf> = paths.into_iter().map(Into::into).collect();
                // Copying a big video takes a moment; do it off the UI thread.
                let spawned = std::thread::Builder::new().name("import".into()).spawn(move || {
                    let results = library::import(&paths);
                    host::post_from_thread(WorkerEvent::ImportDone { results, target });
                });
                if spawned.is_err() {
                    self.post_ui(&json!({ "type": "busy", "on": false }));
                    self.toast("Couldn't start adding the files.", "error");
                }
            }
            FilePurpose::Picture { id, key } => {
                let Some(w) = self.lib.get(&id).cloned() else { return };
                let Some(def) = w.prop(&key).cloned() else { return };
                match library::add_to_folder(&w, &def, std::path::Path::new(&paths[0])) {
                    Ok(name) => {
                        self.set_property(&id, &key, &json!(name));
                        let v = self.props_json(&id);
                        self.post_ui(&v);
                    }
                    Err(e) => self.toast(&e, "error"),
                }
            }
        }
    }

    fn import_done(&mut self, results: Vec<Result<String, String>>, target: &str) {
        self.post_ui(&json!({ "type": "busy", "on": false }));
        self.reload_library();
        let added: Vec<String> = results.iter().filter_map(|r| r.as_ref().ok().cloned()).collect();
        let errors: Vec<String> = results.iter().filter_map(|r| r.as_ref().err().cloned()).collect();
        if let Some(first) = added.first() {
            if !target.is_empty() {
                self.apply(target, Some(first.clone()));
            } else {
                self.rebuild();
            }
            self.post_ui(&json!({ "type": "imported", "id": first }));
        } else {
            self.send_state();
        }
        if !errors.is_empty() {
            for e in &errors {
                log!("couldn't add {e}");
            }
            self.toast(&errors.join("\n"), "error");
        } else if added.len() > 1 {
            self.toast(&format!("Added {} wallpapers.", added.len()), "ok");
        } else if added.len() == 1 {
            let title = self.lib.get(&added[0]).map(|w| w.title.clone()).unwrap_or_default();
            self.toast(&format!("Added {title}."), "ok");
        }
    }
}

fn on_battery() -> bool {
    let mut st = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut st).is_ok() && st.ACLineStatus == 0 }
}
