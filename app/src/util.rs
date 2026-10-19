// Small helpers shared by the rest of the app: paths, the log file, strings for Windows, atomic writes.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use windows::Win32::System::SystemInformation::GetLocalTime;

pub const APP_NAME: &str = "Live Wallpaper";
pub const APP_DIR_NAME: &str = "LiveWallpaper";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Debug builds only: LIVEWALLPAPER_DATA_DIR points the app at a separate folder for testing, so a test run never
/// touches the real settings, wallpapers or Start-with-Windows entry.
pub fn test_data_dir() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var_os("LIVEWALLPAPER_DATA_DIR").filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// %LOCALAPPDATA%\LiveWallpaper, where everything the app writes lives.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = test_data_dir() {
        let _ = fs::create_dir_all(&dir);
        return dir;
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join(APP_DIR_NAME);
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A UTF-16 buffer from Windows, up to its first NUL.
pub fn from_wide(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Write a file through a temp file and a rename, so nobody ever reads half of it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// Parse JSON that may start with a byte order mark (Notepad likes to add one).
pub fn parse_json(bytes: &[u8]) -> Option<serde_json::Value> {
    let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    serde_json::from_slice(b).ok()
}

/// A JavaScript string literal for any text.
pub fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Lower-case letters, digits and single dashes, for folder names and host names.
pub fn slug(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= max {
            break;
        }
    }
    out.trim_matches('-').to_string()
}

pub fn fnv32(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for c in s.encode_utf16() {
        h ^= c as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// Decode %XX escapes in a URL path. Bad escapes are kept as they are.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encode a path for a URL, keeping slashes.
pub fn percent_encode_path(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------- the log ----------
// Problems go to %LOCALAPPDATA%\LiveWallpaper\app.log, so there's something to look at when a wallpaper misbehaves.

static LOG: Mutex<Option<fs::File>> = Mutex::new(None);

pub fn init_log() {
    let path = data_dir().join("app.log");
    // Keep it small: start over once it passes half a megabyte.
    if fs::metadata(&path).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = fs::rename(&path, data_dir().join("app.old.log"));
    }
    if let Ok(f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        *LOG.lock().unwrap() = Some(f);
    }
}

pub fn log_line(msg: &str) {
    let t = unsafe { GetLocalTime() };
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}  {}\r\n",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, msg
    );
    if let Ok(mut g) = LOG.lock() {
        if let Some(f) = g.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }
    #[cfg(debug_assertions)]
    eprint!("{line}");
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::util::log_line(&format!($($arg)*)) };
}
