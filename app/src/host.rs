// The hidden window the app runs around, and the event queue.
//
// Everything that touches the app's state happens on this one thread. Callbacks from Windows and WebView2 never
// change the state directly: they queue an Event and the queue is drained with the App borrowed once at a time.
// That keeps re-entrant calls (a modal menu, a dialog, a COM call that pumps messages) from ever tripping over
// the state. Other threads hand over WorkerEvents through a locked queue and a posted message.

use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{
        atomic::{AtomicIsize, Ordering},
        Mutex,
    },
};

use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2Controller, ICoreWebView2Environment};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Power::POWERBROADCAST_SETTING, SystemServices::GUID_CONSOLE_DISPLAY_STATE},
        UI::WindowsAndMessaging::*,
    },
};

use crate::app::App;

pub const WM_APP_EVENTS: u32 = WM_APP + 1;
pub const WM_APP_WORKER: u32 = WM_APP + 2;
pub const WM_APP_TRAY: u32 = WM_APP + 3;
/// Sent by a second copy of the app, asking this one to open its window.
pub const WM_APP_SHOW: u32 = WM_APP + 4;
/// Sent by `LiveWallpaper.exe --quit`.
pub const WM_APP_QUIT: u32 = WM_APP + 5;

pub const HOST_CLASS: PCWSTR = w!("LiveWallpaperHost");

pub mod timer {
    pub const POLL: usize = 1;
    pub const SAVE: usize = 2;
    pub const DISPLAYS: usize = 3;
    pub const REBUILD: usize = 4;
    pub const AUDIO: usize = 5;
    pub const HELPERS: usize = 6;
    pub const PEEK: usize = 7;
    pub const QUICK: usize = 8;
}

pub enum Event {
    EnvironmentReady(windows::core::Result<ICoreWebView2Environment>),
    SurfaceController { uid: u64, result: windows::core::Result<ICoreWebView2Controller> },
    SurfaceNavigated { uid: u64, ok: bool },
    SurfaceCrashed { uid: u64, why: String },
    /// The WebView2 browser process exited: every page is gone and the engine has to be started again.
    BrowserGone,
    ManagerController(windows::core::Result<ICoreWebView2Controller>),
    ManagerMessage { json: String, files: Vec<String> },
    ManagerCloseRequested,
    ManagerNavigated,
    Tray { msg: u32 },
    TrayCommand(u32),
    Timer(usize),
    DisplaysChanged,
    ExplorerRestarted,
    SessionLocked(bool),
    ScreenPower(bool),
    PowerChanged,
    ThemeChanged,
    ShowManager,
    FilesChosen { purpose: FilePurpose, paths: Vec<String> },
    /// Script to run in the wallpaper inside this window (mouse wheel and "the cursor left" from input.rs).
    PageScript { window: HWND, js: String },
    Worker(WorkerEvent),
    EndSession,
}

#[derive(Clone, Debug)]
pub enum FilePurpose {
    /// Add wallpapers, then put the first on this screen ("all" for every screen).
    Import { target: String },
    /// A picture for a folderDropdown setting.
    Picture { id: String, key: String },
}

pub enum WorkerEvent {
    ImportDone { results: Vec<Result<String, String>>, target: String },
    EarsReply { token: u64, body: String },
}

/// Something to run after the App is released: menus and dialogs, which pump messages while they're open.
pub type Deferred = Box<dyn FnOnce()>;

thread_local! {
    static QUEUE: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
    pub static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

static HOST: AtomicIsize = AtomicIsize::new(0);
static WORKER_QUEUE: Mutex<VecDeque<WorkerEvent>> = Mutex::new(VecDeque::new());
static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn host() -> HWND {
    HWND(HOST.load(Ordering::Relaxed) as *mut _)
}

/// Queue an event from the UI thread. It runs as soon as the App is free.
pub fn post(ev: Event) {
    let first = QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        q.push_back(ev);
        q.len() == 1
    });
    if first {
        unsafe {
            let _ = PostMessageW(Some(host()), WM_APP_EVENTS, WPARAM(0), LPARAM(0));
        }
    }
}

/// Queue an event and handle it now if nothing else is being handled.
pub fn dispatch(ev: Event) {
    QUEUE.with(|q| q.borrow_mut().push_back(ev));
    drain();
}

/// Hand an event over from another thread.
pub fn post_from_thread(ev: WorkerEvent) {
    if let Ok(mut q) = WORKER_QUEUE.lock() {
        q.push_back(ev);
    }
    unsafe {
        let _ = PostMessageW(Some(host()), WM_APP_WORKER, WPARAM(0), LPARAM(0));
    }
}

pub fn drain() {
    loop {
        let Some(ev) = QUEUE.with(|q| q.borrow_mut().pop_front()) else { return };
        let deferred = APP.with(|a| match a.try_borrow_mut() {
            Ok(mut guard) => match guard.as_mut() {
                Some(app) => Ok(app.handle(ev)),
                None => Ok(None),
            },
            Err(_) => Err(ev),
        });
        match deferred {
            Ok(Some(f)) => f(),
            Ok(None) => {}
            Err(ev) => {
                // The App is busy further up this call stack; it drains the rest when it's done.
                QUEUE.with(|q| q.borrow_mut().push_front(ev));
                return;
            }
        }
    }
}

pub fn create_window() -> windows::core::Result<HWND> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: HOST_CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        // A real (never shown) top-level window, not a message-only one: only those hear TaskbarCreated,
        // which is how we learn that Explorer restarted.
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            HOST_CLASS,
            w!("Live Wallpaper"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            None,
        )?;
        HOST.store(hwnd.0 as isize, Ordering::Relaxed);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        Ok(hwnd)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_APP_EVENTS => {
            drain();
            LRESULT(0)
        }
        WM_APP_WORKER => {
            let items: Vec<WorkerEvent> = WORKER_QUEUE.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
            for ev in items {
                QUEUE.with(|q| q.borrow_mut().push_back(Event::Worker(ev)));
            }
            drain();
            LRESULT(0)
        }
        WM_APP_TRAY => {
            // NOTIFYICON_VERSION_4: the event is in the low word of lParam.
            dispatch(Event::Tray { msg: (lp.0 as u32) & 0xFFFF });
            LRESULT(0)
        }
        WM_APP_SHOW => {
            dispatch(Event::ShowManager);
            LRESULT(0)
        }
        WM_APP_QUIT => {
            dispatch(Event::TrayCommand(crate::tray::CMD_QUIT));
            LRESULT(0)
        }
        WM_INPUT => {
            crate::input::on_raw_input(lp);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_TIMER => {
            dispatch(Event::Timer(wp.0));
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            dispatch(Event::DisplaysChanged);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            // The work area changes when the taskbar moves or resizes; the theme when light/dark mode flips.
            if wp.0 as u32 == SPI_SETWORKAREA.0 {
                dispatch(Event::DisplaysChanged);
            } else if lp.0 != 0 {
                if PCWSTR(lp.0 as *const u16).to_string().unwrap_or_default() == "ImmersiveColorSet" {
                    dispatch(Event::ThemeChanged);
                }
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_WTSSESSION_CHANGE => {
            const WTS_SESSION_LOCK: usize = 7;
            const WTS_SESSION_UNLOCK: usize = 8;
            match wp.0 {
                WTS_SESSION_LOCK => dispatch(Event::SessionLocked(true)),
                WTS_SESSION_UNLOCK => dispatch(Event::SessionLocked(false)),
                _ => {}
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            const PBT_APMPOWERSTATUSCHANGE: usize = 0xA;
            const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
            const PBT_POWERSETTINGCHANGE: usize = 0x8013;
            match wp.0 {
                PBT_APMPOWERSTATUSCHANGE => dispatch(Event::PowerChanged),
                PBT_APMRESUMEAUTOMATIC => dispatch(Event::DisplaysChanged),
                PBT_POWERSETTINGCHANGE if lp.0 != 0 => {
                    let s = &*(lp.0 as *const POWERBROADCAST_SETTING);
                    if s.PowerSetting == GUID_CONSOLE_DISPLAY_STATE && s.DataLength >= 4 {
                        let state = *(s.Data.as_ptr() as *const u32);
                        // 0 off, 1 on, 2 dimmed (still visible)
                        dispatch(Event::ScreenPower(state != 0));
                    }
                }
                _ => {}
            }
            LRESULT(1)
        }
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if wp.0 != 0 {
                dispatch(Event::EndSession);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            dispatch(Event::ExplorerRestarted);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Called by Windows when a window moves, minimises or comes to the front, so a newly uncovered desktop
/// starts moving again right away instead of at the next poll.
pub unsafe extern "system" fn win_event(
    _hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if event == EVENT_OBJECT_LOCATIONCHANGE {
        // Only whole top-level windows, not the caret or the cursor.
        if id_object != OBJID_WINDOW.0 || id_child != 0 || hwnd.is_invalid() || GetAncestor(hwnd, GA_ROOT) != hwnd {
            return;
        }
    }
    let _ = SetTimer(Some(host()), timer::QUICK, 80, None);
}
