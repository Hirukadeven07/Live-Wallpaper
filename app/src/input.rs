// Passing the mouse to the wallpapers.
//
// Wallpapers sit behind the desktop icons, so Windows never sends them mouse input. The app listens to the raw
// mouse (without taking it from anyone), and while the cursor is over the bare desktop it posts ordinary mouse
// messages to the wallpaper's browser window, which turns them into normal mousemove/click/wheel events.
// A press that starts on the desktop keeps going to the wallpaper until it's released, so drags and throws work.

use std::cell::{Cell, RefCell};

use windows::{
    core::BOOL,
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
        UI::{
            Input::{
                GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
                RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEMOUSE,
            },
            WindowsAndMessaging::*,
        },
    },
};

use crate::desktop;

#[derive(Clone)]
pub struct Target {
    /// The wallpaper's screen, in screen pixels.
    pub rect: RECT,
    /// The wallpaper's own window; the browser's input window is found inside it.
    pub window: HWND,
}

struct State {
    targets: Vec<Target>,
    /// The browser input window for each target (found lazily, refreshed when it goes away).
    inner: Vec<HWND>,
    /// Which target a press started on, while any button is held.
    captured: Option<usize>,
    buttons: u32,
    last_pos: POINT,
    last_move_ms: u32,
    last_down: Option<(u32, POINT, u32)>, // (button message, where, when)
    over: Option<usize>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        targets: Vec::new(),
        inner: Vec::new(),
        captured: None,
        buttons: 0,
        last_pos: POINT { x: i32::MIN, y: i32::MIN },
        last_move_ms: 0,
        last_down: None,
        over: None,
    });
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

pub fn register(host: HWND) {
    if REGISTERED.with(|r| r.get()) {
        return;
    }
    let dev = RAWINPUTDEVICE {
        usUsagePage: 0x01, // generic desktop
        usUsage: 0x02,     // mouse
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: host,
    };
    unsafe {
        if RegisterRawInputDevices(&[dev], std::mem::size_of::<RAWINPUTDEVICE>() as u32).is_ok() {
            REGISTERED.with(|r| r.set(true));
        } else {
            crate::log!("couldn't listen to the mouse, so wallpapers won't react to it");
        }
    }
}

/// The wallpapers that should get the mouse right now (playing, on screen). Empty turns forwarding off.
pub fn set_targets(targets: Vec<Target>) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.inner = vec![HWND::default(); targets.len()];
        s.targets = targets;
        s.captured = None;
        s.over = None;
    });
}

fn find_inner(window: HWND) -> HWND {
    // Chromium takes input in Chrome_RenderWidgetHostHWND (which hands it to its parent), or failing that in
    // the innermost Chrome_WidgetWin_1.
    struct Find {
        render: HWND,
        widget: HWND,
    }
    unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> BOOL {
        let f = &mut *(lp.0 as *mut Find);
        let cls = desktop::class_name(h);
        if cls == "Chrome_RenderWidgetHostHWND" {
            f.render = h;
            return false.into();
        }
        if cls == "Chrome_WidgetWin_1" {
            f.widget = h;
        }
        true.into()
    }
    let mut f = Find { render: HWND::default(), widget: HWND::default() };
    unsafe {
        let _ = EnumChildWindows(Some(window), Some(cb), LPARAM(&mut f as *mut _ as isize));
    }
    if !f.render.is_invalid() { f.render } else { f.widget }
}

fn inner_of(s: &mut State, i: usize) -> HWND {
    let h = s.inner[i];
    if !h.is_invalid() && unsafe { IsWindow(Some(h)).as_bool() } {
        return h;
    }
    let found = find_inner(s.targets[i].window);
    s.inner[i] = found;
    found
}

fn contains(r: &RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

const LEFT: u32 = 0x0001; // MK_LBUTTON
const RIGHT: u32 = 0x0002; // MK_RBUTTON
const MIDDLE: u32 = 0x0010; // MK_MBUTTON

pub fn on_raw_input(lp: LPARAM) {
    let mut raw = RAWINPUT::default();
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    let got = unsafe {
        GetRawInputData(
            HRAWINPUT(lp.0 as *mut _),
            RID_INPUT,
            Some(&mut raw as *mut _ as *mut _),
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if got == u32::MAX || got == 0 || raw.header.dwType != RIM_TYPEMOUSE.0 {
        return;
    }
    let (flags, data) = unsafe {
        let b = raw.data.mouse.Anonymous.Anonymous;
        (b.usButtonFlags as u32, b.usButtonData as i16)
    };
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.targets.is_empty() {
            return;
        }
        handle(&mut s, flags, data);
    });
}

fn handle(s: &mut State, flags: u32, wheel: i16) {
    let mut pt = POINT::default();
    if unsafe { GetCursorPos(&mut pt) }.is_err() {
        return;
    }
    let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount() };
    let downs = flags & (0x1 | 0x4 | 0x10);
    let ups = flags & (0x2 | 0x8 | 0x20);
    let wheel_v = flags & 0x400 != 0;
    let wheel_h = flags & 0x800 != 0;

    // Who gets this? A held press stays with the wallpaper it started on.
    let target = match s.captured {
        Some(i) => Some(i),
        None => {
            let i = s.targets.iter().position(|t| contains(&t.rect, pt));
            match i {
                Some(i) if desktop::is_desktop_at(pt) => Some(i),
                _ => None,
            }
        }
    };
    // The cursor went onto an app, or onto another screen's wallpaper: tell the one it left.
    if let Some(prev) = s.over {
        if target != Some(prev) {
            s.over = None;
            page_script(&s.targets[prev], "window.__liveWallpaper&&window.__liveWallpaper.leave();".into());
        }
    }
    let Some(i) = target else {
        // Keep track of buttons released elsewhere, so a later press starts clean.
        s.buttons &= !(if ups & 0x2 != 0 { LEFT } else { 0 } | if ups & 0x8 != 0 { RIGHT } else { 0 } | if ups & 0x20 != 0 { MIDDLE } else { 0 });
        return;
    };
    s.over = Some(i);
    let h = inner_of(s, i);
    if h.is_invalid() {
        return;
    }
    let mut client = pt;
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(h, &mut client);
    }
    let lp_client = LPARAM((((client.y as u16) as u32) << 16 | (client.x as u16) as u32) as isize);
    let post = |msg: u32, wp: usize, lp: LPARAM| unsafe {
        let _ = PostMessageW(Some(h), msg, WPARAM(wp), lp);
    };

    let moved = pt.x != s.last_pos.x || pt.y != s.last_pos.y;
    // Moves are capped at about 120 a second; a mouse can report 1000.
    if moved && (downs | ups != 0 || now.wrapping_sub(s.last_move_ms) >= 8) {
        post(WM_MOUSEMOVE, s.buttons as usize, lp_client);
        s.last_pos = pt;
        s.last_move_ms = now;
    }

    for (bit_down, bit_up, mk, down_msg, up_msg, dbl_msg) in [
        (0x1, 0x2, LEFT, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK),
        (0x4, 0x8, RIGHT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_RBUTTONDBLCLK),
        (0x10, 0x20, MIDDLE, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MBUTTONDBLCLK),
    ] {
        if downs & bit_down != 0 {
            s.buttons |= mk;
            // A second press soon after, close by, is a double-click, as Windows itself would send it.
            let dbl = match s.last_down {
                Some((m, p, t)) => {
                    m == down_msg
                        && now.wrapping_sub(t) <= unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() }
                        && (p.x - pt.x).abs() <= unsafe { GetSystemMetrics(SM_CXDOUBLECLK) } / 2
                        && (p.y - pt.y).abs() <= unsafe { GetSystemMetrics(SM_CYDOUBLECLK) } / 2
                }
                None => false,
            };
            if dbl {
                post(dbl_msg, s.buttons as usize, lp_client);
                s.last_down = None;
            } else {
                post(down_msg, s.buttons as usize, lp_client);
                s.last_down = Some((down_msg, pt, now));
            }
        }
        if ups & bit_up != 0 && s.buttons & mk != 0 {
            s.buttons &= !mk;
            post(up_msg, s.buttons as usize, lp_client);
        }
    }

    if wheel_v || wheel_h {
        // Chromium won't take a posted wheel message while the cursor is over another program's window (the
        // desktop icons are), so the page gets the wheel event from a script instead.
        page_script(
            &s.targets[i],
            format!(
                "window.__liveWallpaper&&window.__liveWallpaper.wheel({},{},{},{});",
                client.x, client.y, wheel, wheel_h
            ),
        );
    }

    s.captured = if s.buttons != 0 { Some(i) } else { None };
}

fn page_script(t: &Target, js: String) {
    crate::host::post(crate::host::Event::PageScript { window: t.window, js });
}

/// Runs in every wallpaper page before its own scripts (see surface.rs).
///
/// Chromium thinks the cursor left the page right after every forwarded move (the real cursor is over the
/// desktop icons, not over the browser window), and wallpapers take a "mouseout" without a relatedTarget to mean
/// the cursor has gone. So those are held back here, and the app says when the cursor really leaves.
pub const PAGE_SHIM: &str = r#"(() => {
  if (window.__liveWallpaper) return;
  let letThrough = false;
  const hold = e => { if (!letThrough && !e.relatedTarget) e.stopImmediatePropagation(); };
  for (const t of ['mouseout', 'mouseleave', 'pointerout', 'pointerleave']) window.addEventListener(t, hold, true);
  const fire = (target, type, Ctor, init) => { try { target.dispatchEvent(new Ctor(type, init)); } catch (e) {} };
  window.__liveWallpaper = {
    app: 'Live Wallpaper',
    leave() {
      const root = document.documentElement;
      if (!root) return;
      const o = { bubbles: true, cancelable: true, composed: true, relatedTarget: null, view: window };
      letThrough = true;
      try {
        fire(root, 'pointerout', PointerEvent, o);
        fire(root, 'mouseout', MouseEvent, o);
        fire(root, 'pointerleave', PointerEvent, Object.assign({}, o, { bubbles: false }));
        fire(root, 'mouseleave', MouseEvent, Object.assign({}, o, { bubbles: false }));
        fire(document, 'mouseleave', MouseEvent, Object.assign({}, o, { bubbles: false }));
      } finally { letThrough = false; }
    },
    wheel(x, y, delta, horizontal) {
      const r = window.devicePixelRatio || 1;
      x /= r; y /= r;
      const t = document.elementFromPoint(x, y) || document.body || document.documentElement;
      if (!t) return;
      const d = -delta / 120 * 100;
      fire(t, 'wheel', WheelEvent, {
        bubbles: true, cancelable: true, composed: true, view: window, clientX: x, clientY: y, screenX: x, screenY: y,
        deltaX: horizontal ? -d : 0, deltaY: horizontal ? 0 : d, deltaMode: 0,
      });
    },
  };
})();"#;
