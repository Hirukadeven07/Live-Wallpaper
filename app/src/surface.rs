// One wallpaper playing on one screen: a window in the desktop's wallpaper layer with a WebView2 inside.
//
// The page is served from http://<host>.wp.localhost/ by our own handler (see serve.rs), which keeps each
// wallpaper's storage separate. It talks to the page the way Lively Wallpaper does:
//   livelyPropertyListener(name, value)     each setting at start-up and whenever it changes
//   livelyWallpaperPlaybackChanged(json)    {"IsPaused":true|false} when the desktop is covered or uncovered
//   livelyAudioListener(array)              128 levels about 30 times a second, if the wallpaper asks for sound
// So a wallpaper made for Lively works here unchanged, and the other way round.

use std::{
    cell::Cell,
    path::PathBuf,
    rc::Rc,
    time::Instant,
};

use serde_json::Value;
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, CreateCoreWebView2ControllerCompletedHandler, DownloadStartingEventHandler,
    ExecuteScriptCompletedHandler,
    Microsoft::Web::WebView2::Win32::*, NavigationCompletedEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, ProcessFailedEventHandler, ScriptDialogOpeningEventHandler,
    TrySuspendCompletedHandler, WebResourceRequestedEventHandler,
};
use windows::{
    core::{w, Interface, HSTRING, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{BeginPaint, EndPaint, FillRect, GetStockObject, BLACK_BRUSH, HBRUSH, PAINTSTRUCT},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
};

use crate::{
    desktop::{self, Monitor},
    ears, eyes,
    host::{self, Event},
    library::{Helper, Wallpaper},
    log, serve, util,
};

const CLASS: PCWSTR = w!("LiveWallpaperSurface");

pub struct Surface {
    pub uid: u64,
    pub display: String,
    pub wallpaper: String,
    pub hwnd: HWND,
    pub layer: HWND,
    /// Where it is, in screen pixels.
    pub rect: RECT,
    /// Which screen it's on, in Clawd's Eyes' order (shared with the request handler).
    pub screen_index: Rc<Cell<i32>>,
    pub controller: Option<ICoreWebView2Controller>,
    pub webview: Option<ICoreWebView2>,
    pub loaded: bool,
    pub audio: bool,
    /// Told the page it's paused.
    pub paused: bool,
    /// Hidden and frozen to save memory and power while nobody can see it.
    pub suspended: bool,
    pub hidden_since: Option<Instant>,
    pub crashes: Vec<Instant>,
    tokens: Vec<i64>,
}

fn register_class() {
    unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                FillRect(hdc, &ps.rcPaint, HBRUSH(GetStockObject(BLACK_BRUSH).0));
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
    thread_local!(static DONE: Cell<bool> = const { Cell::new(false) });
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    }
}

impl Surface {
    /// Make the window and start creating the browser in it. The browser arrives as Event::SurfaceController.
    pub fn create(
        env: &ICoreWebView2Environment,
        layer: HWND,
        monitor: &Monitor,
        screen_index: i32,
        w: &Wallpaper,
        uid: u64,
    ) -> windows::core::Result<Surface> {
        register_class();
        let r = desktop::to_layer(layer, monitor.rect);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                CLASS,
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                Some(layer),
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?
        };
        unsafe {
            // Behind anything else in the layer (on systems where that's Progman itself, behind the icons).
            let _ = SetWindowPos(hwnd, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |hr, controller| {
            let result = hr.and_then(|_| controller.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER)));
            host::post(Event::SurfaceController { uid, result });
            Ok(())
        }));
        if let Err(e) = unsafe { env.CreateCoreWebView2Controller(hwnd, &handler) } {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Err(e);
        }
        Ok(Surface {
            uid,
            display: monitor.id.clone(),
            wallpaper: w.id.clone(),
            hwnd,
            layer,
            rect: monitor.rect,
            screen_index: Rc::new(Cell::new(screen_index)),
            controller: None,
            webview: None,
            loaded: false,
            audio: w.audio,
            paused: false,
            suspended: false,
            hidden_since: None,
            crashes: Vec::new(),
            tokens: Vec::new(),
        })
    }

    /// The browser is ready: set it up, hook up its requests, and open the wallpaper.
    pub fn attach(
        &mut self,
        env: &ICoreWebView2Environment,
        controller: ICoreWebView2Controller,
        w: &Wallpaper,
        developer: bool,
    ) -> windows::core::Result<()> {
        let uid = self.uid;
        unsafe {
            let webview = controller.CoreWebView2()?;
            let settings = webview.Settings()?;
            settings.SetAreDefaultContextMenusEnabled(false)?;
            settings.SetAreDevToolsEnabled(developer)?;
            settings.SetIsStatusBarEnabled(false)?;
            settings.SetIsZoomControlEnabled(false)?;
            settings.SetAreDefaultScriptDialogsEnabled(false)?;
            settings.SetIsBuiltInErrorPageEnabled(true)?;
            if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
                let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
            }
            if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
                let _ = s4.SetIsPasswordAutosaveEnabled(false);
                let _ = s4.SetIsGeneralAutofillEnabled(false);
            }
            if let Ok(s5) = settings.cast::<ICoreWebView2Settings5>() {
                let _ = s5.SetIsPinchZoomEnabled(false);
            }
            if let Ok(s6) = settings.cast::<ICoreWebView2Settings6>() {
                let _ = s6.SetIsSwipeNavigationEnabled(false);
            }
            if let Ok(s8) = settings.cast::<ICoreWebView2Settings8>() {
                // No SmartScreen look-ups: the wallpaper is a local page.
                let _ = s8.SetIsReputationCheckingRequired(false);
            }
            if let Ok(c2) = controller.cast::<ICoreWebView2Controller2>() {
                let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 255, R: 0, G: 0, B: 0 });
            }

            // Requests for the wallpaper's own files, and for the helpers' data.
            let ctx = Requests {
                env: env.clone(),
                root: w.dir.clone(),
                host: format!("{}.wp.localhost", w.host),
                helper: w.helper,
                screen: self.screen_index.clone(),
            };
            webview.AddWebResourceRequestedFilter(
                &HSTRING::from(format!("http://{}/*", ctx.host)),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            )?;
            if w.helper == Helper::Ears {
                webview.AddWebResourceRequestedFilter(
                    &HSTRING::from(format!("http://127.0.0.1:{}/*", ears::PORT)),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                )?;
            }
            let mut token = 0i64;
            webview.add_WebResourceRequested(
                &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        ctx.answer(&args);
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            self.tokens.push(token);

            webview.add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                    let ok = args
                        .map(|a| {
                            let mut b = windows::core::BOOL(0);
                            a.IsSuccess(&mut b).is_ok() && b.as_bool()
                        })
                        .unwrap_or(false);
                    host::post(Event::SurfaceNavigated { uid, ok });
                    Ok(())
                })),
                &mut token,
            )?;
            webview.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    let kind = args.and_then(|a| {
                        let mut k = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                        a.ProcessFailedKind(&mut k).ok().map(|_| k)
                    });
                    // Only the page's own process going away needs a reload; helper processes restart themselves.
                    let matters = matches!(
                        kind,
                        Some(COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED)
                            | Some(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED)
                            | Some(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE)
                            | Some(COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED)
                    );
                    if kind == Some(COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED) {
                        // The whole engine went away, not just this page.
                        host::post(Event::BrowserGone);
                    } else if matters {
                        host::post(Event::SurfaceCrashed { uid, why: format!("{:?}", kind.map(|k| k.0)) });
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            // A wallpaper never opens windows, asks for the camera, or downloads things.
            webview.add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                    if let Some(a) = args {
                        let _ = a.SetHandled(true);
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            webview.add_PermissionRequested(
                &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                    if let Some(a) = args {
                        let _ = a.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY);
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            webview.add_ScriptDialogOpening(
                &ScriptDialogOpeningEventHandler::create(Box::new(|_, _| Ok(()))),
                &mut token,
            )?;
            if let Ok(w4) = webview.cast::<ICoreWebView2_4>() {
                let _ = w4.add_DownloadStarting(
                    &DownloadStartingEventHandler::create(Box::new(|_, args| {
                        if let Some(a) = args {
                            let _ = a.SetCancel(true);
                        }
                        Ok(())
                    })),
                    &mut token,
                );
            }

            // Before the page's own scripts: the mouse helpers (see input.rs).
            webview.AddScriptToExecuteOnDocumentCreated(
                &HSTRING::from(crate::input::PAGE_SHIM),
                &AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(|_, _| Ok(()))),
            )?;

            let mut client = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut client);
            controller.SetBounds(client)?;
            controller.SetIsVisible(true)?;
            webview.Navigate(&HSTRING::from(w.url()))?;
            self.webview = Some(webview);
            self.controller = Some(controller);
        }
        Ok(())
    }

    /// Run a bit of script in the page (fire and forget).
    pub fn exec(&self, js: &str) {
        if let Some(wv) = &self.webview {
            unsafe {
                let _ = wv.ExecuteScript(&HSTRING::from(js), &ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(()))));
            }
        }
    }

    /// Send settings to the page's livelyPropertyListener.
    pub fn send_properties(&self, values: &[(String, Value)]) {
        if values.is_empty() {
            return;
        }
        let mut js = String::from("(function(){var f=window.livelyPropertyListener;if(typeof f!=='function')return;");
        for (k, v) in values {
            js.push_str(&format!("try{{f({},{});}}catch(e){{console.error(e);}}", util::js_str(k), v));
        }
        js.push_str("})();");
        self.exec(&js);
    }

    pub fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        if self.loaded {
            self.send_playback();
        }
    }

    fn send_playback(&self) {
        let arg = util::js_str(&format!("{{\"IsPaused\":{}}}", self.paused));
        self.exec(&format!(
            "window.livelyWallpaperPlaybackChanged&&window.livelyWallpaperPlaybackChanged({arg});"
        ));
    }

    /// Hide and freeze the page, and let the browser give memory back. Only while nobody can see it.
    pub fn suspend(&mut self) {
        if self.suspended {
            return;
        }
        let (Some(c), Some(wv)) = (&self.controller, &self.webview) else { return };
        self.suspended = true;
        unsafe {
            let _ = c.SetIsVisible(false);
            if let Ok(w19) = wv.cast::<ICoreWebView2_19>() {
                let _ = w19.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
            }
            if let Ok(w3) = wv.cast::<ICoreWebView2_3>() {
                let _ = w3.TrySuspend(&TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(()))));
            }
        }
    }

    pub fn resume(&mut self) {
        if !self.suspended {
            return;
        }
        self.suspended = false;
        let (Some(c), Some(wv)) = (&self.controller, &self.webview) else { return };
        unsafe {
            if let Ok(w19) = wv.cast::<ICoreWebView2_19>() {
                let _ = w19.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL);
            }
            if let Ok(w3) = wv.cast::<ICoreWebView2_3>() {
                let _ = w3.Resume();
            }
            let _ = c.SetIsVisible(true);
        }
    }

    pub fn reload(&mut self) {
        if let Some(wv) = &self.webview {
            self.loaded = false;
            unsafe {
                let _ = wv.Reload();
            }
        }
    }

    pub fn open_devtools(&self) {
        if let Some(wv) = &self.webview {
            unsafe {
                if let Ok(s) = wv.Settings() {
                    let _ = s.SetAreDevToolsEnabled(true);
                }
                let _ = wv.OpenDevToolsWindow();
            }
        }
    }

    pub fn set_developer(&self, on: bool) {
        if let Some(wv) = &self.webview {
            unsafe {
                if let Ok(s) = wv.Settings() {
                    let _ = s.SetAreDevToolsEnabled(on);
                }
            }
        }
    }

    /// Still in the right place, in a window that still exists?
    pub fn alive(&self, layer: HWND) -> bool {
        unsafe { self.layer == layer && IsWindow(Some(self.hwnd)).as_bool() && GetParent(self.hwnd).ok() == Some(layer) }
    }

    pub fn destroy(mut self) {
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

/// What the page's request handler needs. It runs on the UI thread but never touches the app's state.
struct Requests {
    env: ICoreWebView2Environment,
    root: PathBuf,
    host: String,
    helper: Helper,
    screen: Rc<Cell<i32>>,
}

impl Requests {
    fn answer(&self, args: &ICoreWebView2WebResourceRequestedEventArgs) {
        let Some(req) = (unsafe { args.Request() }).ok().and_then(|r| serve::read_request(&r)) else { return };
        let host = serve::url_host(&req.uri);
        let resp = if self.helper == Helper::Ears && host == format!("127.0.0.1:{}", ears::PORT) {
            // Clawd Tunes' player buttons. Answered later if Spotify has to be asked first.
            ears::answer_http(&self.env, args, &req.uri, &req.method)
        } else if host == self.host {
            let path = serve::url_path(&req.uri);
            let path = if path.is_empty() { "index.html".to_string() } else { path };
            match (self.helper, path.as_str()) {
                (Helper::Eyes, "clawd-eyes-data.js") => {
                    serve::bytes_response(&self.env, 200, "text/javascript; charset=utf-8", "", eyes::data_js(self.screen.get()).as_bytes())
                }
                (Helper::Ears, "clawd-tunes-data.js") => {
                    serve::bytes_response(&self.env, 200, "text/javascript; charset=utf-8", "", ears::data_js().as_bytes())
                }
                (Helper::Ears, "clawd-tunes-art.js") => {
                    serve::bytes_response(&self.env, 200, "text/javascript; charset=utf-8", "", ears::art_js().as_bytes())
                }
                _ => match serve::safe_join(&self.root, &path) {
                    Some(file) => serve::file_response(&self.env, &file, &req),
                    None => serve::not_found(&self.env),
                },
            }
        } else {
            return;
        };
        if let Some(resp) = resp {
            unsafe {
                if let Err(e) = args.SetResponse(&resp) {
                    log!("couldn't answer {}: {e}", req.uri);
                }
            }
        }
    }
}
