// Answering a page's requests ourselves: wallpaper files from disk, the app's own page, and the helpers' data.
// WebView2 hands each request to a WebResourceRequested handler; these build the responses.

use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use webview2_com::{
    take_pwstr,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment, ICoreWebView2WebResourceRequest, ICoreWebView2WebResourceResponse,
    },
};
use windows::{
    core::{HSTRING, PWSTR},
    Win32::{
        System::Com::{IStream, STGM_READ, STGM_SHARE_DENY_NONE},
        UI::Shell::{SHCreateMemStream, SHCreateStreamOnFileEx},
    },
};

use crate::util;

/// Files up to this size are read into memory in one go; bigger ones stream from disk.
const SMALL: u64 = 8 << 20;
/// The most one range request gets back, so seeking around a long video never loads all of it.
const RANGE_CHUNK: u64 = 8 << 20;

pub fn mime(path: &Path) -> &'static str {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "csv" => "text/csv; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        _ => "application/octet-stream",
    }
}

pub struct Request {
    pub uri: String,
    pub method: String,
    pub range: Option<String>,
}

pub fn read_request(req: &ICoreWebView2WebResourceRequest) -> Option<Request> {
    unsafe {
        let mut uri = PWSTR::null();
        req.Uri(&mut uri).ok()?;
        let uri = take_pwstr(uri);
        let mut method = PWSTR::null();
        let method = if req.Method(&mut method).is_ok() { take_pwstr(method) } else { "GET".into() };
        let range = req.Headers().ok().and_then(|h| {
            let mut v = PWSTR::null();
            h.GetHeader(&HSTRING::from("Range"), &mut v).ok()?;
            let s = take_pwstr(v);
            (!s.is_empty()).then_some(s)
        });
        Some(Request { uri, method, range })
    }
}

/// The path part of a URL, decoded, without the query or fragment: "http://x.wp.localhost/a%20b.png?1" -> "a b.png".
pub fn url_path(uri: &str) -> String {
    let rest = uri.split_once("://").map(|(_, r)| r).unwrap_or(uri);
    let path = rest.find('/').map(|i| &rest[i + 1..]).unwrap_or("");
    let path = path.split(['?', '#']).next().unwrap_or("");
    util::percent_decode(path)
}

/// The host of a URL, lower-cased.
pub fn url_host(uri: &str) -> String {
    let rest = uri.split_once("://").map(|(_, r)| r).unwrap_or(uri);
    rest.split(['/', '?', '#']).next().unwrap_or("").to_lowercase()
}

/// A file under `root` for a URL path, refusing anything that would step outside it.
pub fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut p = root.to_path_buf();
    for part in rel.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            s if s.contains(':') => return None,
            s => p.push(s),
        }
    }
    Some(p)
}

pub fn respond(
    env: &ICoreWebView2Environment,
    status: i32,
    reason: &str,
    headers: &str,
    body: Option<&IStream>,
) -> Option<ICoreWebView2WebResourceResponse> {
    unsafe {
        env.CreateWebResourceResponse(body, status, &HSTRING::from(reason), &HSTRING::from(headers)).ok()
    }
}

pub fn bytes_response(
    env: &ICoreWebView2Environment,
    status: i32,
    content_type: &str,
    extra_headers: &str,
    bytes: &[u8],
) -> Option<ICoreWebView2WebResourceResponse> {
    let stream = unsafe { SHCreateMemStream(Some(bytes)) }?;
    let headers = format!(
        "Content-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{extra_headers}",
        bytes.len()
    );
    let reason = if status == 200 { "OK" } else { "Error" };
    respond(env, status, reason, headers.trim_end(), Some(&stream))
}

pub fn not_found(env: &ICoreWebView2Environment) -> Option<ICoreWebView2WebResourceResponse> {
    respond(env, 404, "Not Found", "Content-Type: text/plain\r\nCache-Control: no-store", None)
}

/// A file from disk, with byte ranges for media (videos need them to seek and loop).
pub fn file_response(env: &ICoreWebView2Environment, path: &Path, req: &Request) -> Option<ICoreWebView2WebResourceResponse> {
    let Ok(meta) = fs::metadata(path) else { return not_found(env) };
    if !meta.is_file() {
        return not_found(env);
    }
    let size = meta.len();
    let ty = mime(path);
    // "bytes=START-END", "bytes=START-" or "bytes=-SUFFIX"
    if let Some(range) = req.range.as_deref().and_then(|r| r.trim().strip_prefix("bytes=")) {
        if let Some((a, b)) = range.split(',').next().unwrap_or("").split_once('-') {
            let (start, end) = match (a.trim().parse::<u64>().ok(), b.trim().parse::<u64>().ok()) {
                (Some(s), Some(e)) => (s, e.min(size.saturating_sub(1))),
                (Some(s), None) => (s, size.saturating_sub(1)),
                (None, Some(suffix)) => (size.saturating_sub(suffix), size.saturating_sub(1)),
                (None, None) => (0, size.saturating_sub(1)),
            };
            if size == 0 || start >= size || start > end {
                let h = format!("Content-Range: bytes */{size}\r\nContent-Type: {ty}");
                return respond(env, 416, "Range Not Satisfiable", &h, None);
            }
            let end = end.min(start + RANGE_CHUNK - 1);
            let mut buf = vec![0u8; (end - start + 1) as usize];
            let ok = fs::File::open(path)
                .and_then(|mut f| {
                    f.seek(SeekFrom::Start(start))?;
                    f.read_exact(&mut buf)
                })
                .is_ok();
            if !ok {
                return not_found(env);
            }
            let stream = unsafe { SHCreateMemStream(Some(&buf)) }?;
            let h = format!(
                "Content-Type: {ty}\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{size}\r\nAccept-Ranges: bytes\r\nCache-Control: no-cache",
                buf.len()
            );
            return respond(env, 206, "Partial Content", &h, Some(&stream));
        }
    }
    let headers = format!("Content-Type: {ty}\r\nContent-Length: {size}\r\nAccept-Ranges: bytes\r\nCache-Control: no-cache");
    if req.method.eq_ignore_ascii_case("HEAD") {
        return respond(env, 200, "OK", &headers, None);
    }
    let stream: IStream = if size <= SMALL {
        let Ok(bytes) = fs::read(path) else { return not_found(env) };
        unsafe { SHCreateMemStream(Some(&bytes)) }?
    } else {
        let wpath = HSTRING::from(path.as_os_str());
        match unsafe {
            SHCreateStreamOnFileEx(&wpath, (STGM_READ | STGM_SHARE_DENY_NONE).0, 0, false, None)
        } {
            Ok(s) => s,
            Err(_) => return not_found(env),
        }
    };
    respond(env, 200, "OK", &headers, Some(&stream))
}
