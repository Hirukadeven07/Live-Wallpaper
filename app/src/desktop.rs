// The Windows desktop: the screens, the layer between the wallpaper and the icons where wallpapers go, and whether
// anything is covering a screen right now.
//
// The layer: sending Progman the undocumented 0x052C message makes Explorer split the desktop into the icons and a
// "WorkerW" window behind them. Windows 11 24H2 and later keep that WorkerW inside Progman; older versions make it a
// separate top-level window right after the one holding the icons. Wallpaper windows become children of it.

use windows::{
    core::{w, BOOL, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS},
            Gdi::{
                EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, MapWindowPoints, HDC, HMONITOR,
                MONITORINFO, MONITORINFOEXW, DISPLAY_DEVICEW,
            },
        },
        System::Threading::GetCurrentProcessId,
        UI::{
            HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
            WindowsAndMessaging::*,
        },
    },
};

use crate::util::{from_wide, wide};

#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// Stable id for the screen (its device path), used to remember which wallpaper goes where.
    pub id: String,
    /// "Main screen", "Screen 2"...
    pub name: String,
    pub rect: RECT,
    pub work: RECT,
    pub dpi: u32,
    pub primary: bool,
}

const EDD_GET_DEVICE_INTERFACE_NAME: u32 = 1;

/// All screens: the main one first, then left to right, then top to bottom. (Clawd's Eyes uses the same order.)
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn cb(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<Monitor>);
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(h, &mut mi as *mut _ as *mut MONITORINFO).as_bool() {
            return true.into();
        }
        let device = from_wide(&mi.szDevice);
        let mut dd = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        let dev_w = wide(&device);
        let id = if EnumDisplayDevicesW(PCWSTR(dev_w.as_ptr()), 0, &mut dd, EDD_GET_DEVICE_INTERFACE_NAME).as_bool() {
            let s = from_wide(&dd.DeviceID);
            if s.is_empty() { device.clone() } else { s }
        } else {
            device.clone()
        };
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        list.push(Monitor {
            id,
            name: String::new(),
            rect: mi.monitorInfo.rcMonitor,
            work: mi.monitorInfo.rcWork,
            dpi: dx,
            primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
        true.into()
    }
    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut list as *mut _ as isize));
    }
    list.sort_by(|a, b| {
        b.primary
            .cmp(&a.primary)
            .then(a.rect.left.cmp(&b.rect.left))
            .then(a.rect.top.cmp(&b.rect.top))
    });
    // Two screens of the same model can report the same path; keep ids unique.
    for i in 0..list.len() {
        let mut n = 2;
        let base = list[i].id.clone();
        while list[..i].iter().any(|m| m.id == list[i].id) {
            list[i].id = format!("{base}#{n}");
            n += 1;
        }
        list[i].name = if i == 0 { "Main screen".to_string() } else { format!("Screen {}", i + 1) };
    }
    list
}

pub fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The window wallpapers go in, behind the desktop icons. None if Explorer isn't running.
pub fn wallpaper_layer() -> Option<HWND> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        let mut res = 0usize;
        // Ask Explorer to create the layer behind the icons. It's harmless to ask again.
        let _ = SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(0x1), SMTO_NORMAL, 1000, Some(&mut res));
        let _ = SendMessageTimeoutW(progman, 0x052C, WPARAM(0), LPARAM(0), SMTO_NORMAL, 1000, Some(&mut res));

        // Windows 11 24H2 and later: the layer is a WorkerW inside Progman.
        if let Ok(w) = FindWindowExW(Some(progman), None, w!("WorkerW"), PCWSTR::null()) {
            if !w.is_invalid() {
                return Some(w);
            }
        }
        // Older: the WorkerW right after the top-level window that holds the icons (SHELLDLL_DefView).
        unsafe extern "system" fn find(top: HWND, data: LPARAM) -> BOOL {
            let out = &mut *(data.0 as *mut HWND);
            if let Ok(view) = FindWindowExW(Some(top), None, w!("SHELLDLL_DefView"), PCWSTR::null()) {
                if !view.is_invalid() {
                    if let Ok(worker) = FindWindowExW(None, Some(top), w!("WorkerW"), PCWSTR::null()) {
                        if !worker.is_invalid() {
                            *out = worker;
                            return false.into();
                        }
                    }
                }
            }
            true.into()
        }
        let mut found = HWND::default();
        let _ = EnumWindows(Some(find), LPARAM(&mut found as *mut _ as isize));
        if !found.is_invalid() {
            return Some(found);
        }
        // Explorer didn't split the desktop (it happens on some setups): fall back to Progman itself.
        Some(progman)
    }
}

/// A screen rectangle in the layer's own coordinates.
pub fn to_layer(layer: HWND, r: RECT) -> RECT {
    let mut pts = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
    unsafe {
        MapWindowPoints(None, Some(layer), &mut pts);
    }
    RECT { left: pts[0].x, top: pts[0].y, right: pts[1].x, bottom: pts[1].y }
}

/// Is the window under this point part of the desktop (the icons or the wallpaper), rather than an app?
pub fn is_desktop_at(pt: POINT) -> bool {
    unsafe {
        let hwnd = WindowFromPoint(pt);
        if hwnd.is_invalid() {
            return false;
        }
        let root = GetAncestor(hwnd, GA_ROOT);
        let cls = class_name(if root.is_invalid() { hwnd } else { root });
        cls == "Progman" || cls == "WorkerW"
    }
}

/// For each screen: is a window covering all of it (a maximised or full-screen app)?
pub fn covered(monitors: &[Monitor]) -> Vec<bool> {
    struct Scan {
        pid: u32,
        rects: Vec<RECT>,
    }
    unsafe extern "system" fn cb(hwnd: HWND, data: LPARAM) -> BOOL {
        let scan = &mut *(data.0 as *mut Scan);
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return true.into();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == scan.pid {
            return true.into();
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & (WS_EX_TRANSPARENT.0 | WS_EX_TOOLWINDOW.0) != 0 {
            return true.into();
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4).is_ok() && cloaked != 0 {
            return true.into();
        }
        let cls = class_name(hwnd);
        if matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return true.into();
        }
        let mut r = RECT::default();
        if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32)
            .is_err()
            && GetWindowRect(hwnd, &mut r).is_err()
        {
            return true.into();
        }
        if r.right - r.left > 100 && r.bottom - r.top > 100 {
            scan.rects.push(r);
        }
        true.into()
    }
    let mut scan = Scan { pid: unsafe { GetCurrentProcessId() }, rects: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut scan as *mut _ as isize));
    }
    monitors
        .iter()
        .map(|m| {
            scan.rects.iter().any(|r| {
                r.left <= m.work.left && r.top <= m.work.top && r.right >= m.work.right && r.bottom >= m.work.bottom
            })
        })
        .collect()
}

/// Make Explorer repaint the normal wallpaper after ours goes away.
pub fn restore_windows_wallpaper() {
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETDESKWALLPAPER, SPI_SETDESKWALLPAPER};
    unsafe {
        let mut buf = [0u16; 1024];
        let got = SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            Some(buf.as_mut_ptr() as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        if got.is_ok() {
            // Setting the same picture again (without saving anything) makes Explorer redraw it.
            let _ = SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, Some(buf.as_mut_ptr() as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        }
    }
}
