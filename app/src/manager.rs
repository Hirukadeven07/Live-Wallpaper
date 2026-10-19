// The app's window: a WebView2 showing the page in app/ui (wallpaper picker and settings).
// It only exists while it's open; closing it frees its browser process, so the app idles with no UI loaded.
// The page talks to the app with chrome.webview.postMessage; the app answers with PostWebMessageAsJson.

use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc};

use webview2_com::{
    take_pwstr, CreateCoreWebView2ControllerCompletedHandler, Microsoft::Web::WebView2::Win32::*,
    NavigationCompletedEventHandler, NewWindowRequestedEventHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler,
};
use windows::{
    core::{w, Interface, HSTRING, PCWSTR, PWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE},
            Gdi::{CreateSolidBrush, MonitorFromPoint, MONITOR_DEFAULTTONEAREST},
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD},
        },
        UI::{
            HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI},
            WindowsAndMessaging::*,
        },
    },
};

use crate::{
    host::{self, Event},
    serve, util,
};

const CLASS: PCWSTR = w!("LiveWallpaperManager");
pub const UI_HOST: &str = "app.ui.localhost";

static UI_HTML: &str = include_str!("../ui/index.html");
static UI_CSS: &str = include_str!("../ui/app.css");
static UI_JS: &str = include_str!("../ui/app.js");
static UI_ICON: &[u8] = include_bytes!("../ui/icon.png");

thread_local! {
    /// The window's browser, for resizing it from the window procedure without touching the App.
    static CONTROLLER: RefCell<Option<ICoreWebView2Controller>> = const { RefCell::new(None) };
}

pub type Folders = Rc<RefCell<HashMap<String, PathBuf>>>;

pub struct Manager {
    pub hwnd: HWND,
    pub webview: Option<ICoreWebView2>,
    controller: Option<ICoreWebView2Controller>,
    /// Messages for the page that arrived before it was ready.
    queue: Vec<String>,
    pub ready: bool,
}

pub fn dark_mode() -> bool {
    unsafe {
        let mut v: u32 = 1;
        let mut size = 4u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut _ as *mut _),
            Some(&mut size),
        );
        r.is_ok() && v == 0
    }
}

fn background(dark: bool) -> (u8, u8, u8) {
    if dark { (0x1a, 0x19, 0x18) } else { (0xfa, 0xf9, 0xf5) }
}

fn register_class() {
    thread_local!(static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let (r, g, b) = background(dark_mode());
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: LoadIconW(Some(hinst.into()), PCWSTR(1 as *const u16)).unwrap_or_default(),
            hbrBackground: CreateSolidBrush(windows::Win32::Foundation::COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    }
}

pub fn apply_theme(hwnd: HWND) {
    let dark: i32 = dark_mode() as i32;
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as *const _, 4);
    }
    CONTROLLER.with(|c| {
        if let Some(c) = c.borrow().as_ref() {
            if let Ok(c2) = c.cast::<ICoreWebView2Controller2>() {
                let (r, g, b) = background(dark != 0);
                unsafe {
                    let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 255, R: r, G: g, B: b });
                }
            }
        }
    });
}

impl Manager {
    pub fn open(env: &ICoreWebView2Environment) -> windows::core::Result<Manager> {
        register_class();
        unsafe {
            // Centre it on the screen with the cursor, at a comfortable size for that screen's scaling.
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let mut mi = windows::Win32::Graphics::Gdi::MONITORINFO {
                cbSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = windows::Win32::Graphics::Gdi::GetMonitorInfoW(mon, &mut mi);
            let work = mi.rcWork;
            let k = dx as f64 / 96.0;
            let w = ((1180.0 * k) as i32).min(work.right - work.left - 40);
            let h = ((780.0 * k) as i32).min(work.bottom - work.top - 40);
            let x = work.left + (work.right - work.left - w) / 2;
            let y = work.top + (work.bottom - work.top - h) / 2;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                w!("Live Wallpaper"),
                WS_OVERLAPPEDWINDOW,
                x,
                y,
                w,
                h,
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?;
            apply_theme(hwnd);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(|hr, controller| {
                let result = hr.and_then(|_| controller.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER)));
                host::post(Event::ManagerController(result));
                Ok(())
            }));
            if let Err(e) = env.CreateCoreWebView2Controller(hwnd, &handler) {
                let _ = DestroyWindow(hwnd);
                return Err(e);
            }
            Ok(Manager { hwnd, webview: None, controller: None, queue: Vec::new(), ready: false })
        }
    }

    pub fn attach(
        &mut self,
        env: &ICoreWebView2Environment,
        controller: ICoreWebView2Controller,
        folders: Folders,
        developer: bool,
    ) -> windows::core::Result<()> {
        unsafe {
            let webview = controller.CoreWebView2()?;
            let settings = webview.Settings()?;
            settings.SetAreDevToolsEnabled(developer || cfg!(debug_assertions))?;
            settings.SetAreDefaultContextMenusEnabled(cfg!(debug_assertions))?;
            settings.SetIsStatusBarEnabled(false)?;
            settings.SetIsZoomControlEnabled(false)?;
            if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
                let _ = s3.SetAreBrowserAcceleratorKeysEnabled(cfg!(debug_assertions));
            }
            if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
                let _ = s4.SetIsGeneralAutofillEnabled(false);
                let _ = s4.SetIsPasswordAutosaveEnabled(false);
            }
            if let Ok(s8) = settings.cast::<ICoreWebView2Settings8>() {
                let _ = s8.SetIsReputationCheckingRequired(false);
            }
            if let Ok(c2) = controller.cast::<ICoreWebView2Controller2>() {
                let (r, g, b) = background(dark_mode());
                let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 255, R: r, G: g, B: b });
            }

            webview.AddWebResourceRequestedFilter(&HSTRING::from(format!("http://{UI_HOST}/*")), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
            let env2 = env.clone();
            let mut token = 0i64;
            webview.add_WebResourceRequested(
                &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let Some(req) = args.Request().ok().and_then(|r| serve::read_request(&r)) else { return Ok(()) };
                    if serve::url_host(&req.uri) != UI_HOST {
                        return Ok(());
                    }
                    let path = serve::url_path(&req.uri);
                    let text = |ty: &str, body: &str| serve::bytes_response(&env2, 200, ty, "", body.as_bytes());
                    let resp = match path.as_str() {
                        "" | "index.html" => text("text/html; charset=utf-8", UI_HTML),
                        "app.css" => text("text/css; charset=utf-8", UI_CSS),
                        "app.js" => text("text/javascript; charset=utf-8", UI_JS),
                        "icon.png" => serve::bytes_response(&env2, 200, "image/png", "", UI_ICON),
                        p if p.starts_with("wp/") => {
                            // wp/<wallpaper id>/<file>: thumbnails, pictures and videos from a wallpaper's folder.
                            let rest = &p[3..];
                            let (id, file) = rest.split_once('/').unwrap_or((rest, ""));
                            let root = folders.borrow().get(id).cloned();
                            match root.and_then(|r| serve::safe_join(&r, file)) {
                                Some(f) => serve::file_response(&env2, &f, &req),
                                None => serve::not_found(&env2),
                            }
                        }
                        _ => serve::not_found(&env2),
                    };
                    if let Some(r) = resp {
                        let _ = args.SetResponse(&r);
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            webview.add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(|_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut s = PWSTR::null();
                    if args.WebMessageAsJson(&mut s).is_err() {
                        return Ok(());
                    }
                    let json = take_pwstr(s);
                    // Files dropped on the page come along with the message, with their real paths.
                    let mut files = Vec::new();
                    if let Ok(a2) = args.cast::<ICoreWebView2WebMessageReceivedEventArgs2>() {
                        if let Ok(objs) = a2.AdditionalObjects() {
                            let mut n = 0u32;
                            let _ = objs.Count(&mut n);
                            for i in 0..n {
                                if let Ok(o) = objs.GetValueAtIndex(i) {
                                    if let Ok(f) = o.cast::<ICoreWebView2File>() {
                                        let mut p = PWSTR::null();
                                        if f.Path(&mut p).is_ok() {
                                            files.push(take_pwstr(p));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    host::post(Event::ManagerMessage { json, files });
                    Ok(())
                })),
                &mut token,
            )?;
            webview.add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(|_, _| {
                    host::post(Event::ManagerNavigated);
                    Ok(())
                })),
                &mut token,
            )?;
            // Links in the page open in the real browser.
            webview.add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                    if let Some(a) = args {
                        let mut uri = PWSTR::null();
                        if a.Uri(&mut uri).is_ok() {
                            let u = take_pwstr(uri);
                            if u.starts_with("https://") {
                                crate::dialogs::open(&u);
                            }
                        }
                        let _ = a.SetHandled(true);
                    }
                    Ok(())
                })),
                &mut token,
            )?;

            let mut r = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut r);
            controller.SetBounds(r)?;
            controller.SetIsVisible(true)?;
            webview.Navigate(&HSTRING::from(format!("http://{UI_HOST}/index.html")))?;
            let _ = controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            CONTROLLER.with(|c| *c.borrow_mut() = Some(controller.clone()));
            self.webview = Some(webview);
            self.controller = Some(controller);
        }
        Ok(())
    }

    /// The page loaded: send what queued up meanwhile.
    pub fn loaded(&mut self) {
        self.ready = true;
        for m in std::mem::take(&mut self.queue) {
            self.post(&m);
        }
    }

    pub fn post(&mut self, json: &str) {
        match (&self.webview, self.ready) {
            (Some(wv), true) => unsafe {
                let _ = wv.PostWebMessageAsJson(&HSTRING::from(json));
            },
            _ => self.queue.push(json.to_string()),
        }
    }

    pub fn focus(&self) {
        unsafe {
            if IsIconic(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_RESTORE);
            }
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    pub fn set_developer(&self, on: bool) {
        if let Some(wv) = &self.webview {
            unsafe {
                if let Ok(s) = wv.Settings() {
                    let _ = s.SetAreDevToolsEnabled(on || cfg!(debug_assertions));
                }
            }
        }
    }

    pub fn close(mut self) {
        CONTROLLER.with(|c| *c.borrow_mut() = None);
        unsafe {
            if let Some(c) = self.controller.take() {
                let _ = c.Close();
            }
            self.webview = None;
            if IsWindow(Some(self.hwnd)).as_bool() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            let c = CONTROLLER.with(|c| c.borrow().clone());
            if let Some(c) = c {
                let mut r = RECT::default();
                let _ = GetClientRect(hwnd, &mut r);
                let _ = c.SetBounds(r);
            }
            LRESULT(0)
        }
        WM_MOVE | WM_MOVING => {
            let c = CONTROLLER.with(|c| c.borrow().clone());
            if let Some(c) = c {
                let _ = c.NotifyParentWindowPositionChanged();
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_SETFOCUS => {
            let c = CONTROLLER.with(|c| c.borrow().clone());
            if let Some(c) = c {
                let _ = c.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let mmi = &mut *(lp.0 as *mut MINMAXINFO);
            let dpi = GetDpiForWindow(hwnd).max(96) as i32;
            mmi.ptMinTrackSize.x = 760 * dpi / 96;
            mmi.ptMinTrackSize.y = 520 * dpi / 96;
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let r = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            if lp.0 != 0 && PCWSTR(lp.0 as *const u16).to_string().unwrap_or_default() == "ImmersiveColorSet" {
                apply_theme(hwnd);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CLOSE => {
            host::dispatch(Event::ManagerCloseRequested);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Encode a wallpaper id for a wp/<id>/ address in the page.
pub fn id_for_url(id: &str) -> String {
    let mut out = String::new();
    for b in id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn file_url(id: &str, rel: &str) -> String {
    format!("http://{UI_HOST}/wp/{}/{}", id_for_url(id), util::percent_encode_path(rel))
}
