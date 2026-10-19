// Live Wallpaper: plays animated web wallpapers behind the Windows desktop icons.
//
//   LiveWallpaper.exe               start (or bring up the window if it's already running)
//   LiveWallpaper.exe --background  start without opening the window (how Windows starts it at sign-in)
//   LiveWallpaper.exe --quit        close the copy that's running

#![windows_subsystem = "windows"]

mod app;
mod audio;
mod config;
mod desktop;
mod dialogs;
mod ears;
mod eyes;
mod host;
mod input;
mod library;
mod manager;
mod serve;
mod surface;
mod tray;
mod util;

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, LPARAM, WPARAM},
        System::{
            Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
            Threading::CreateMutexW,
        },
        UI::WindowsAndMessaging::{
            AllowSetForegroundWindow, DispatchMessageW, FindWindowW, GetMessageW, PostMessageW, TranslateMessage, ASFW_ANY,
            MSG,
        },
    },
};

fn main() {
    util::init_log();
    std::panic::set_hook(Box::new(|info| {
        log!("crashed: {info}");
    }));
    let has = |flag: &str| std::env::args().any(|a| a.eq_ignore_ascii_case(flag));
    let background = has("--background");
    let quit = has("--quit");

    // One copy at a time. A second start just opens the first one's window (or, with --quit, closes it).
    let mutex = unsafe { CreateMutexW(None, true, w!("Local\\LiveWallpaper.SingleInstance")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS || quit {
        unsafe {
            if let Ok(h) = FindWindowW(host::HOST_CLASS, PCWSTR::null()) {
                if quit {
                    let _ = PostMessageW(Some(h), host::WM_APP_QUIT, WPARAM(0), LPARAM(0));
                } else {
                    let _ = AllowSetForegroundWindow(ASFW_ANY);
                    let _ = PostMessageW(Some(h), host::WM_APP_SHOW, WPARAM(0), LPARAM(0));
                }
            }
        }
        return;
    }

    log!("starting {} {}", util::APP_NAME, util::VERSION);
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    library::prepare_builtins();

    let hwnd = match host::create_window() {
        Ok(h) => h,
        Err(e) => {
            log!("couldn't create the app's window: {e}");
            return;
        }
    };
    host::APP.with(|a| *a.borrow_mut() = Some(app::App::new(hwnd, !background)));
    host::APP.with(|a| {
        if let Some(app) = a.borrow_mut().as_mut() {
            app.start();
        }
    });
    host::drain();

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    host::APP.with(|a| a.borrow_mut().take());
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
        CoUninitialize();
        if let Ok(m) = mutex {
            let _ = CloseHandle(m);
        }
    }
    log!("stopped");
}
