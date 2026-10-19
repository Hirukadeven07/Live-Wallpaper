// The icon in the system tray: click it to open the app, right-click for a short menu.

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{HWND, POINT},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{LoadIconMetric, LIM_SMALL},
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
                NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
            },
            WindowsAndMessaging::*,
        },
    },
};

use crate::host::WM_APP_TRAY;

pub const CMD_OPEN: u32 = 1;
pub const CMD_PAUSE: u32 = 2;
pub const CMD_AUTOSTART: u32 = 3;
pub const CMD_QUIT: u32 = 4;

fn data(host: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: host,
        uID: 1,
        ..Default::default()
    };
    nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    nid
}

fn set_tip(nid: &mut NOTIFYICONDATAW, tip: &str) {
    let w: Vec<u16> = tip.encode_utf16().take(nid.szTip.len() - 1).collect();
    nid.szTip = [0; 128];
    nid.szTip[..w.len()].copy_from_slice(&w);
}

pub fn add(host: HWND, tip: &str) {
    unsafe {
        let mut nid = data(host);
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        if let Ok(hinst) = GetModuleHandleW(None) {
            // Icon 1 in the exe, at the tray's size for this screen's scaling.
            if let Ok(icon) = LoadIconMetric(Some(hinst.into()), PCWSTR(1 as *const u16), LIM_SMALL) {
                nid.hIcon = icon;
            }
        }
        set_tip(&mut nid, tip);
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
}

pub fn set_tooltip(host: HWND, tip: &str) {
    unsafe {
        let mut nid = data(host);
        nid.uFlags = NIF_TIP | NIF_SHOWTIP;
        set_tip(&mut nid, tip);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn remove(host: HWND) {
    unsafe {
        let nid = data(host);
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

/// Show the right-click menu at the cursor. Returns the chosen command, or 0.
/// This runs its own message loop, so it must not be called while the App is borrowed.
pub fn menu(host: HWND, paused: bool, autostart: bool) -> u32 {
    unsafe {
        let Ok(m) = CreatePopupMenu() else { return 0 };
        let item = |id: u32, text: &str, checked: bool| {
            let w = crate::util::wide(text);
            let flags = MF_STRING | if checked { MF_CHECKED } else { MF_UNCHECKED };
            let _ = AppendMenuW(m, flags, id as usize, PCWSTR(w.as_ptr()));
        };
        item(CMD_OPEN, "Open Live Wallpaper", false);
        let _ = SetMenuDefaultItem(m, CMD_OPEN, 0);
        item(CMD_PAUSE, "Pause wallpapers", paused);
        item(CMD_AUTOSTART, "Start with Windows", autostart);
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        item(CMD_QUIT, "Quit", false);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // The menu only closes on an outside click if our window is in front while it's open.
        let _ = SetForegroundWindow(host);
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pt.x, pt.y, None, host, None);
        let _ = PostMessageW(Some(host), WM_NULL, Default::default(), Default::default());
        let _ = DestroyMenu(m);
        cmd.0 as u32
    }
}
