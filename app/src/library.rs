// The wallpapers the app knows about.
//
// Built-in ones are packed into the exe and unpacked to %LOCALAPPDATA%\LiveWallpaper\Library\builtin on first run
// (and again whenever the app brings newer versions). The user's own live in Library\user, one folder each, in the
// same format Lively Wallpaper uses: an index.html, plus an optional LivelyInfo.json (title, description...) and
// LivelyProperties.json (the settings). Zips, single web pages, pictures and videos can all be added.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde_json::{json, Map, Value};

use crate::{log, util};

mod packed {
    include!(concat!(env!("OUT_DIR"), "/builtin.rs"));
}

/// The order built-ins appear in, newest ideas last. Anything not listed goes after these.
const BUILTIN_ORDER: &[&str] = &[
    "claude-dunes",
    "clawd-dunes",
    "clawd-desk-buddy",
    "clawd-desk-stations",
    "clawd-tunes",
];

/// Built-ins that play with the mouse, so the app can say so.
const USES_MOUSE: &[&str] = &["clawd-desk-buddy", "clawd-desk-stations", "clawd-tunes", "clawd-dunes"];

const IMAGE_EXT: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp", "avif", "svg"];
const VIDEO_EXT: &[&str] = &["mp4", "webm", "mov", "m4v", "ogv"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Helper {
    None,
    /// Clawd's Eyes: the desktop icons, for Clawd Desk Stations.
    Eyes,
    /// Clawd's Ears: what Spotify is playing, for Clawd Tunes.
    Ears,
}

#[derive(Clone, Debug)]
pub enum Source {
    /// A page inside the wallpaper's folder (relative path, forward slashes).
    Page(String),
    /// A website.
    Url(String),
}

#[derive(Clone, Debug)]
pub struct Wallpaper {
    pub id: String,
    /// The wallpaper's own web address is http://<host>.wp.localhost/, so each one keeps its own storage.
    pub host: String,
    pub dir: PathBuf,
    pub builtin: bool,
    pub title: String,
    pub desc: String,
    pub author: String,
    pub source: Source,
    pub audio: bool,
    pub thumbnail: Option<String>,
    pub preview: Option<String>,
    /// Settings from LivelyProperties.json, in file order: key -> its definition.
    pub props: Vec<(String, Value)>,
    pub helper: Helper,
    pub uses_mouse: bool,
    /// "picture" or "video" for wallpapers made from a single picture or video, otherwise "web".
    pub kind: String,
}

impl Wallpaper {
    pub fn url(&self) -> String {
        match &self.source {
            Source::Page(p) => format!("http://{}.wp.localhost/{}", self.host, util::percent_encode_path(p)),
            Source::Url(u) => u.clone(),
        }
    }

    pub fn prop(&self, key: &str) -> Option<&Value> {
        self.props.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

pub fn library_dir() -> PathBuf {
    util::data_dir().join("Library")
}
pub fn builtin_dir() -> PathBuf {
    library_dir().join("builtin")
}
pub fn user_dir() -> PathBuf {
    let d = library_dir().join("user");
    let _ = fs::create_dir_all(&d);
    d
}

// ---------- the built-ins ----------

/// Unpack the built-in wallpapers if they're missing or older than the ones in this exe.
/// Pictures the user added to a built-in's folder are left alone.
pub fn prepare_builtins() {
    let root = builtin_dir();
    let stamp = root.join(".version");
    let current = fs::read_to_string(&stamp).unwrap_or_default();
    let complete = packed::BUILTIN_FILES
        .iter()
        .all(|(id, rel, _)| root.join(id).join(rel).is_file());
    if current.trim() == packed::BUILTIN_HASH && complete {
        return;
    }
    let mut ok = true;
    for (id, rel, bytes) in packed::BUILTIN_FILES {
        let path = root.join(id).join(rel);
        if fs::read(&path).map(|old| old == *bytes).unwrap_or(false) {
            continue;
        }
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = util::write_atomic(&path, bytes) {
            log!("couldn't unpack {}: {e}", path.display());
            ok = false;
        }
    }
    if ok {
        let _ = fs::write(&stamp, packed::BUILTIN_HASH);
    }
}

fn builtin_ids() -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for (id, _, _) in packed::BUILTIN_FILES {
        if !ids.iter().any(|i| i == id) {
            ids.push(id.to_string());
        }
    }
    ids.sort_by_key(|id| BUILTIN_ORDER.iter().position(|o| o == id).unwrap_or(usize::MAX));
    ids
}

// ---------- reading a wallpaper folder ----------

pub struct Library {
    pub items: Vec<Wallpaper>,
}

impl Library {
    pub fn scan() -> Library {
        let mut items = Vec::new();
        for id in builtin_ids() {
            let dir = builtin_dir().join(&id);
            match read_folder(&dir, id.clone(), id.clone(), true) {
                Ok(w) => items.push(w),
                Err(e) => log!("built-in wallpaper {id} can't be used: {e}"),
            }
        }
        let mut users: Vec<(String, PathBuf)> = fs::read_dir(user_dir())
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                    .collect()
            })
            .unwrap_or_default();
        users.sort_by_key(|(n, _)| n.to_lowercase());
        for (name, dir) in users {
            if name.starts_with('.') {
                continue;
            }
            let id = format!("user:{name}");
            let host = format!("u-{}-{:08x}", util::slug(&name, 40), util::fnv32(&name));
            match read_folder(&dir, id, host, false) {
                Ok(w) => items.push(w),
                Err(e) => log!("skipping {}: {e}", dir.display()),
            }
        }
        Library { items }
    }

    pub fn get(&self, id: &str) -> Option<&Wallpaper> {
        self.items.iter().find(|w| w.id == id)
    }
}

fn read_json(path: &Path) -> Option<Value> {
    fs::read(path).ok().and_then(|b| util::parse_json(&b))
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").trim().to_string()
}

fn read_folder(dir: &Path, id: String, host: String, builtin: bool) -> Result<Wallpaper, String> {
    let info = read_json(&dir.join("LivelyInfo.json")).unwrap_or(Value::Null);
    let folder_name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut title = str_of(&info, "Title");
    if title.is_empty() {
        title = folder_name.clone();
    }
    let kind_no = info.get("Type").and_then(|t| t.as_i64()).unwrap_or(1);
    let file = str_of(&info, "FileName");
    let args = str_of(&info, "Arguments").to_lowercase();
    let source = match kind_no {
        3 => {
            if !(file.starts_with("http://") || file.starts_with("https://")) {
                return Err("its web address isn't an http or https link".into());
            }
            Source::Url(file)
        }
        1 | 2 => {
            let page = if file.is_empty() { "index.html".to_string() } else { file.replace('\\', "/") };
            if page.contains("..") || Path::new(&page).is_absolute() || !dir.join(&page).is_file() {
                return Err(format!("{page} isn't in the wallpaper's folder"));
            }
            Source::Page(page)
        }
        _ => return Err(format!("wallpapers of type {kind_no} (apps, games, raw video) aren't supported")),
    };
    let existing = |key: &str| -> Option<String> {
        let f = str_of(&info, key).replace('\\', "/");
        (!f.is_empty() && !f.contains("..") && dir.join(&f).is_file()).then_some(f)
    };
    let mut props = Vec::new();
    if let Some(Value::Object(map)) = read_json(&dir.join("LivelyProperties.json")) {
        for (k, v) in map {
            if v.is_object() {
                props.push((k, v));
            }
        }
    }
    let kind = info
        .get("LiveWallpaperKind")
        .and_then(|k| k.as_str())
        .unwrap_or("web")
        .to_string();
    let helper = match (builtin, id.as_str()) {
        (true, "clawd-desk-stations") => Helper::Eyes,
        (true, "clawd-tunes") => Helper::Ears,
        _ => Helper::None,
    };
    Ok(Wallpaper {
        uses_mouse: builtin && USES_MOUSE.contains(&id.as_str()),
        id,
        host,
        dir: dir.to_path_buf(),
        builtin,
        title,
        desc: str_of(&info, "Desc"),
        author: str_of(&info, "Author"),
        source,
        audio: kind_no == 2 || args.contains("--audio true"),
        thumbnail: existing("Thumbnail"),
        preview: existing("Preview"),
        props,
        helper,
        kind,
    })
}

// ---------- settings ----------

/// A setting's value in the type Lively would send it as, or None if it doesn't make sense.
pub fn coerce(def: &Value, v: &Value) -> Option<Value> {
    let ty = def.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match ty {
        "slider" => {
            let n = v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))?;
            let min = def.get("min").and_then(|x| x.as_f64()).unwrap_or(f64::MIN);
            let max = def.get("max").and_then(|x| x.as_f64()).unwrap_or(f64::MAX);
            let n = n.clamp(min.min(max), max.max(min));
            let step = def.get("step").and_then(|x| x.as_f64()).unwrap_or(1.0);
            if step.fract() == 0.0 && n.fract() == 0.0 {
                Some(json!(n as i64))
            } else {
                Some(json!(n))
            }
        }
        "checkbox" => v.as_bool().map(Value::Bool),
        "dropdown" => {
            let n = v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))?;
            let count = def.get("items").and_then(|i| i.as_array()).map(|a| a.len() as i64).unwrap_or(0);
            (n >= 0 && (count == 0 || n < count)).then(|| json!(n))
        }
        "color" | "textbox" | "folderDropdown" => v.as_str().map(|s| Value::String(s.to_string())),
        _ => None,
    }
}

/// The value as the wallpaper's livelyPropertyListener gets it.
pub fn dispatch_value(def: &Value, v: &Value) -> Value {
    if def.get("type").and_then(|t| t.as_str()) == Some("folderDropdown") {
        let name = v.as_str().unwrap_or("");
        if name.is_empty() {
            return Value::String(String::new());
        }
        let folder = def.get("folder").and_then(|f| f.as_str()).unwrap_or("");
        // Lively joins the folder and the file with a backslash, so wallpapers expect that.
        return Value::String(if folder.is_empty() { name.to_string() } else { format!("{folder}\\{name}") });
    }
    v.clone()
}

/// Every setting the wallpaper should hear about when it starts, with the user's choices applied.
pub fn startup_values(w: &Wallpaper, user: Option<&Map<String, Value>>) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for (key, def) in &w.props {
        let ty = def.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty == "button" || ty == "label" {
            continue;
        }
        let v = user
            .and_then(|m| m.get(key))
            .and_then(|v| coerce(def, v))
            .or_else(|| def.get("value").cloned());
        if let Some(v) = v {
            out.push((key.clone(), dispatch_value(def, &v)));
        }
    }
    out
}

fn filter_exts(def: &Value) -> Vec<String> {
    def.get("filter")
        .and_then(|f| f.as_str())
        .unwrap_or("*.jpg|*.jpeg|*.png|*.webp|*.bmp|*.gif")
        .split('|')
        .filter_map(|p| p.trim().rsplit('.').next().map(|e| e.to_lowercase()))
        .filter(|e| !e.is_empty() && e != "*")
        .collect()
}

/// The files a folderDropdown setting can pick from.
pub fn folder_files(w: &Wallpaper, def: &Value) -> Vec<String> {
    let folder = def.get("folder").and_then(|f| f.as_str()).unwrap_or("");
    let exts = filter_exts(def);
    let mut files: Vec<String> = fs::read_dir(w.dir.join(folder))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| {
                    let ext = Path::new(n).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                    exts.is_empty() || exts.contains(&ext)
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by_key(|n| n.to_lowercase());
    files
}

/// Copy a picture into a folderDropdown's folder. Returns the name it got there.
pub fn add_to_folder(w: &Wallpaper, def: &Value, src: &Path) -> Result<String, String> {
    let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let exts = filter_exts(def);
    if !exts.is_empty() && !exts.contains(&ext) {
        return Err(format!("This setting takes {} files.", exts.join(", ")));
    }
    let folder = def.get("folder").and_then(|f| f.as_str()).unwrap_or("");
    let dir = w.dir.join(folder);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
    let mut name = format!("{stem}.{ext}");
    let mut n = 2;
    while dir.join(&name).exists() {
        if fs::read(dir.join(&name)).ok() == fs::read(src).ok() {
            return Ok(name); // already there
        }
        name = format!("{stem} ({n}).{ext}");
        n += 1;
    }
    fs::copy(src, dir.join(&name)).map_err(|e| format!("Couldn't copy the picture: {e}"))?;
    Ok(name)
}

// ---------- adding the user's own wallpapers ----------

/// Add files or folders the user picked or dropped. Each gives the new wallpaper's id or a reason it didn't work.
pub fn import(paths: &[PathBuf]) -> Vec<Result<String, String>> {
    paths.iter().map(|p| import_one(p)).collect()
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn display_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

fn import_one(src: &Path) -> Result<String, String> {
    let name = display_name(src);
    let result = if src.is_dir() {
        import_folder(src)
    } else {
        let ext = ext_of(src);
        if ext == "zip" {
            import_zip(src)
        } else if ext == "html" || ext == "htm" {
            import_page(src)
        } else if IMAGE_EXT.contains(&ext.as_str()) || VIDEO_EXT.contains(&ext.as_str()) {
            import_media(src)
        } else {
            Err("That kind of file can't be a wallpaper. Try a .zip, an .html page, a picture or a video.".into())
        }
    };
    result.map_err(|e| format!("{name}: {e}"))
}

/// A new, empty folder in Library\user named after the wallpaper.
fn new_folder(title: &str) -> Result<PathBuf, String> {
    let base = {
        let s = util::slug(title, 40);
        if s.is_empty() { "wallpaper".to_string() } else { s }
    };
    let root = user_dir();
    let mut name = base.clone();
    let mut n = 2;
    while root.join(&name).exists() || BUILTIN_ORDER.contains(&name.as_str()) {
        name = format!("{base}-{n}");
        n += 1;
    }
    let dir = root.join(&name);
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make a folder for it: {e}"))?;
    Ok(dir)
}

fn id_of(dir: &Path) -> String {
    format!("user:{}", dir.file_name().unwrap().to_string_lossy())
}

/// Turn a freshly copied folder into a wallpaper the app can play, or remove it and say why not.
fn finish(dir: PathBuf, fallback_title: &str) -> Result<String, String> {
    let r = finish_inner(&dir, fallback_title);
    if r.is_err() {
        let _ = fs::remove_dir_all(&dir);
    }
    r.map(|_| id_of(&dir))
}

fn finish_inner(dir: &Path, fallback_title: &str) -> Result<(), String> {
    let info_path = dir.join("LivelyInfo.json");
    let mut info = match read_json(&info_path) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let ty = info.get("Type").and_then(|t| t.as_i64()).unwrap_or(1);
    let file = info.get("FileName").and_then(|f| f.as_str()).unwrap_or("").replace('\\', "/");
    let title = info
        .get("Title")
        .and_then(|t| t.as_str())
        .filter(|t| !t.trim().is_empty())
        .map(|t| t.to_string())
        .unwrap_or_else(|| fallback_title.to_string());
    info.insert("Title".into(), json!(title));
    match ty {
        1 | 2 => {
            let page = if file.is_empty() { "index.html".to_string() } else { file };
            if !dir.join(&page).is_file() {
                return Err("there's no index.html (or the page LivelyInfo.json names) in it.".into());
            }
            info.insert("FileName".into(), json!(page));
            info.entry("Type").or_insert(json!(1));
        }
        3 => {
            if !(file.starts_with("http://") || file.starts_with("https://")) {
                return Err("its web address isn't an http or https link.".into());
            }
        }
        7 | 8 | 11 => {
            // A Lively picture, gif or video wallpaper: wrap the file in a little page.
            let media = dir.join(&file);
            if file.is_empty() || file.contains("..") || !media.is_file() {
                return Err("the picture or video it names isn't inside it.".into());
            }
            let is_video = VIDEO_EXT.contains(&ext_of(&media).as_str());
            write_media_page(dir, &file, is_video, &mut info)?;
        }
        _ => return Err("it's an app, game or stream wallpaper. Only web pages, pictures and videos work here.".into()),
    }
    let bytes = serde_json::to_vec_pretty(&Value::Object(info)).unwrap();
    fs::write(&info_path, bytes).map_err(|e| e.to_string())?;
    Ok(())
}

fn import_folder(src: &Path) -> Result<String, String> {
    if !src.join("index.html").is_file() && !src.join("LivelyInfo.json").is_file() {
        return Err("a wallpaper folder needs an index.html (or a LivelyInfo.json) in it.".into());
    }
    let title = display_name(src);
    let dir = new_folder(&title)?;
    if let Err(e) = copy_dir(src, &dir, 0) {
        let _ = fs::remove_dir_all(&dir);
        return Err(format!("couldn't copy it: {e}"));
    }
    finish(dir, &title)
}

fn copy_dir(from: &Path, to: &Path, depth: u32) -> std::io::Result<()> {
    if depth > 16 {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)?.flatten() {
        let ft = e.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        let target = to.join(e.file_name());
        if ft.is_dir() {
            copy_dir(&e.path(), &target, depth + 1)?;
        } else {
            fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

fn import_zip(src: &Path) -> Result<String, String> {
    let f = fs::File::open(src).map_err(|e| format!("couldn't open it: {e}"))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|_| "it isn't a zip file the app can read.".to_string())?;
    // Find the wallpaper inside: the shallowest LivelyInfo.json, or else the shallowest index.html.
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().and_then(|e| e.enclosed_name()).map(|p| p.to_string_lossy().replace('\\', "/")))
        .filter(|n| !n.starts_with("__MACOSX/"))
        .collect();
    let pick = |want: &str| {
        names
            .iter()
            .filter(|n| n.rsplit('/').next().map(|f| f.eq_ignore_ascii_case(want)).unwrap_or(false))
            .min_by_key(|n| n.matches('/').count())
            .map(|n| n[..n.len() - want.len()].to_string())
    };
    let root = pick("LivelyInfo.json")
        .or_else(|| pick("index.html"))
        .ok_or_else(|| "there's no wallpaper in it (no index.html or LivelyInfo.json).".to_string())?;
    let title = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "wallpaper".into());
    let dir = new_folder(&title)?;
    let mut total: u64 = 0;
    let res = (|| -> Result<(), String> {
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let Some(path) = entry.enclosed_name() else { continue };
            let rel = path.to_string_lossy().replace('\\', "/");
            let Some(inner) = rel.strip_prefix(&root) else { continue };
            if inner.is_empty() || inner.starts_with("__MACOSX/") {
                continue;
            }
            let out = dir.join(inner);
            if entry.is_dir() {
                fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                continue;
            }
            total += entry.size();
            if total > 4 << 30 {
                return Err("it's too big (over 4 GB unpacked).".into());
            }
            if let Some(p) = out.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut buf = Vec::with_capacity(entry.size().min(64 << 20) as usize);
            entry.read_to_end(&mut buf).map_err(|e| format!("part of it is damaged ({e})."))?;
            fs::write(&out, buf).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(e) = res {
        let _ = fs::remove_dir_all(&dir);
        return Err(e);
    }
    finish(dir, &title)
}

fn page_title(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let a = lower.find("<title>")? + 7;
    let b = a + lower[a..].find("</title>")?;
    let t = html[a..b].trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn import_page(src: &Path) -> Result<String, String> {
    // A page with a LivelyInfo.json beside it is a whole wallpaper folder.
    if let Some(parent) = src.parent() {
        if parent.join("LivelyInfo.json").is_file() {
            return import_folder(parent);
        }
    }
    let html = fs::read(src).map_err(|e| format!("couldn't read it: {e}"))?;
    let title = page_title(&String::from_utf8_lossy(&html))
        .unwrap_or_else(|| src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    let dir = new_folder(&title)?;
    if let Err(e) = fs::write(dir.join("index.html"), &html) {
        let _ = fs::remove_dir_all(&dir);
        return Err(e.to_string());
    }
    finish(dir, &title)
}

fn import_media(src: &Path) -> Result<String, String> {
    let ext = ext_of(src);
    let is_video = VIDEO_EXT.contains(&ext.as_str());
    let title = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "wallpaper".into());
    let dir = new_folder(&title)?;
    let file = format!("wallpaper.{ext}");
    let res = (|| -> Result<(), String> {
        fs::copy(src, dir.join(&file)).map_err(|e| format!("couldn't copy it: {e}"))?;
        let mut info = Map::new();
        info.insert("Title".into(), json!(title));
        write_media_page(&dir, &file, is_video, &mut info)?;
        fs::write(dir.join("LivelyInfo.json"), serde_json::to_vec_pretty(&Value::Object(info)).unwrap())
            .map_err(|e| e.to_string())
    })();
    if let Err(e) = res {
        let _ = fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(id_of(&dir))
}

/// A page that shows one picture or video full screen, with settings for how it fits.
fn write_media_page(dir: &Path, file: &str, is_video: bool, info: &mut Map<String, Value>) -> Result<(), String> {
    let src = util::percent_encode_path(file);
    let tag = if is_video {
        format!(r#"<video id="m" src="{src}" autoplay loop muted playsinline></video>"#)
    } else {
        format!(r#"<img id="m" src="{src}" alt="">"#)
    };
    let page = MEDIA_PAGE.replace("{{TAG}}", &tag);
    fs::write(dir.join("index.html"), page).map_err(|e| e.to_string())?;
    let mut props = json!({
        "fit": { "type": "dropdown", "value": 0, "text": "Fit", "items": ["Fill the screen", "Fit inside the screen", "Stretch", "Actual size"] },
        "position": { "type": "dropdown", "value": 0, "text": "Position", "items": ["Centre", "Top", "Bottom", "Left", "Right"] },
        "bg": { "type": "color", "value": "#000000", "text": "Colour around it" },
    });
    if is_video {
        props["volume"] = json!({ "type": "slider", "value": 0, "min": 0, "max": 100, "step": 1, "text": "Sound (%)" });
        props["speed"] = json!({ "type": "dropdown", "value": 2, "text": "Speed", "items": ["0.5x", "0.75x", "1x", "1.25x", "1.5x", "2x"] });
    }
    fs::write(dir.join("LivelyProperties.json"), serde_json::to_vec_pretty(&props).unwrap()).map_err(|e| e.to_string())?;
    info.insert("Type".into(), json!(1));
    info.insert("FileName".into(), json!("index.html"));
    info.insert("LiveWallpaperKind".into(), json!(if is_video { "video" } else { "picture" }));
    info.entry("Desc").or_insert(json!(if is_video { "Your video." } else { "Your picture." }));
    if is_video {
        info.insert("Preview".into(), json!(file));
        info.remove("Thumbnail");
    } else {
        info.insert("Thumbnail".into(), json!(file));
    }
    Ok(())
}

const MEDIA_PAGE: &str = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>Wallpaper</title>
<style>
  html, body { margin: 0; height: 100%; overflow: hidden; background: #000; }
  #m { position: fixed; inset: 0; width: 100%; height: 100%; object-fit: cover; object-position: 50% 50%; }
</style>
</head>
<body>
{{TAG}}
<script>
  // Settings come from Live Wallpaper (or Lively Wallpaper) through livelyPropertyListener.
  const m = document.getElementById('m');
  const FIT = ['cover', 'contain', 'fill', 'none'];
  const POS = ['50% 50%', '50% 0%', '50% 100%', '0% 50%', '100% 50%'];
  const isVideo = m.tagName === 'VIDEO';
  window.livelyPropertyListener = (name, val) => {
    if (name === 'fit') m.style.objectFit = FIT[val] || 'cover';
    else if (name === 'position') m.style.objectPosition = POS[val] || '50% 50%';
    else if (name === 'bg') document.body.style.background = val;
    else if (name === 'volume' && isVideo) { m.volume = Math.max(0, Math.min(1, val / 100)); m.muted = val <= 0; }
    else if (name === 'speed' && isVideo) m.playbackRate = [0.5, 0.75, 1, 1.25, 1.5, 2][val] || 1;
  };
  window.livelyWallpaperPlaybackChanged = data => {
    if (!isVideo) return;
    let o = {};
    try { o = typeof data === 'string' ? JSON.parse(data) : data; } catch (e) {}
    if (o && (o.IsPaused || o.isPaused)) m.pause(); else m.play().catch(() => {});
  };
</script>
</body>
</html>
"#;

/// Remove one of the user's wallpapers.
pub fn delete(w: &Wallpaper) -> Result<(), String> {
    if w.builtin {
        return Err("Built-in wallpapers can't be removed.".into());
    }
    if !w.dir.starts_with(user_dir()) {
        return Err("That folder isn't in your wallpapers folder.".into());
    }
    fs::remove_dir_all(&w.dir).map_err(|e| format!("Couldn't remove it: {e}"))
}

/// A starter wallpaper for people who want to make their own.
pub fn create_starter() -> Result<String, String> {
    let dir = new_folder("my-wallpaper")?;
    let files: [(&str, &str); 3] = [
        ("index.html", STARTER_HTML),
        ("LivelyInfo.json", STARTER_INFO),
        ("LivelyProperties.json", STARTER_PROPS),
    ];
    for (name, body) in files {
        if let Err(e) = fs::write(dir.join(name), body) {
            let _ = fs::remove_dir_all(&dir);
            return Err(format!("Couldn't create it: {e}"));
        }
    }
    Ok(id_of(&dir))
}

const STARTER_INFO: &str = r#"{
  "AppVersion": "1.0.0.0",
  "Title": "My wallpaper",
  "Thumbnail": null,
  "Preview": null,
  "Desc": "A starting point for your own wallpaper. Edit index.html, then press Reload.",
  "Author": "",
  "License": "",
  "Contact": "",
  "Type": 1,
  "FileName": "index.html",
  "Arguments": "--pause-event true",
  "IsAbsolutePath": false
}
"#;

const STARTER_PROPS: &str = r##"{
  "colour": {
    "type": "color",
    "value": "#D97757",
    "text": "Colour"
  },
  "count": {
    "type": "slider",
    "value": 60,
    "min": 10,
    "max": 300,
    "step": 10,
    "text": "How many dots"
  },
  "speed": {
    "type": "slider",
    "value": 100,
    "min": 0,
    "max": 300,
    "step": 10,
    "text": "Speed (%)"
  },
  "dark": {
    "type": "checkbox",
    "value": true,
    "text": "Dark background"
  }
}
"##;

const STARTER_HTML: &str = r##"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>My wallpaper</title>
<style>
  html, body { margin: 0; height: 100%; overflow: hidden; background: #141413; }
  canvas { display: block; width: 100vw; height: 100vh; }
</style>
</head>
<body>
<canvas id="c"></canvas>
<script>
/*
  A starter wallpaper for Live Wallpaper. It also works in Lively Wallpaper.

  - Settings: LivelyProperties.json lists them. Each one arrives through livelyPropertyListener(name, value),
    once when the wallpaper starts and again whenever you change it in the app.
    Types: slider (number), checkbox (true/false), dropdown (index of the item), color ("#rrggbb"),
    textbox (text), button (called with true when pressed), folderDropdown (a file inside a folder).
  - Pausing: livelyWallpaperPlaybackChanged('{"IsPaused":true}') arrives when the desktop is covered, so stop
    drawing until it says false. (When it's covered for longer, the app also hides and freezes the page.)
  - Sound: add "--audio true" to Arguments in LivelyInfo.json and livelyAudioListener(array) gets 128 numbers
    from 0 to 1, lowest pitch first, about 30 times a second.
  - The mouse: moves, clicks and the wheel arrive as normal mouse events while the cursor is over the desktop.
*/
const S = { colour: '#D97757', count: 60, speed: 100, dark: true };
const c = document.getElementById('c'), ctx = c.getContext('2d');
let W = 0, H = 0, dpr = 1, dots = [], paused = false, mouse = { x: -1e9, y: -1e9 };

function resize() {
  dpr = devicePixelRatio || 1;
  W = c.width = Math.round(innerWidth * dpr);
  H = c.height = Math.round(innerHeight * dpr);
}
function makeDots() {
  dots = Array.from({ length: S.count }, () => ({
    x: Math.random() * W, y: Math.random() * H, r: (2 + Math.random() * 6) * dpr,
    vx: (Math.random() - 0.5) * 30 * dpr, vy: (Math.random() - 0.5) * 30 * dpr
  }));
}
let last = 0;
function frame(t) {
  requestAnimationFrame(frame);
  if (paused) { last = 0; return; }
  const dt = last ? Math.min(0.1, (t - last) / 1000) : 0;
  last = t;
  ctx.fillStyle = S.dark ? '#141413' : '#F0EEE6';
  ctx.fillRect(0, 0, W, H);
  ctx.fillStyle = S.colour;
  const k = S.speed / 100;
  for (const d of dots) {
    // dots drift away from the mouse
    const dx = d.x - mouse.x, dy = d.y - mouse.y, dist = Math.hypot(dx, dy);
    if (dist < 120 * dpr) { d.vx += dx / dist * 40 * dpr * dt; d.vy += dy / dist * 40 * dpr * dt; }
    d.x = (d.x + d.vx * dt * k + W) % W;
    d.y = (d.y + d.vy * dt * k + H) % H;
    ctx.beginPath(); ctx.arc(d.x, d.y, d.r, 0, Math.PI * 2); ctx.fill();
  }
}

window.livelyPropertyListener = (name, val) => {
  if (name === 'colour') S.colour = val;
  else if (name === 'count') { S.count = val; makeDots(); }
  else if (name === 'speed') S.speed = val;
  else if (name === 'dark') S.dark = !!val;
};
window.livelyWallpaperPlaybackChanged = data => {
  try { paused = !!JSON.parse(data).IsPaused; } catch (e) { paused = false; }
};
addEventListener('mousemove', e => { mouse = { x: e.clientX * dpr, y: e.clientY * dpr }; });
addEventListener('resize', () => { resize(); makeDots(); });
resize(); makeDots();
requestAnimationFrame(frame);
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Each test gets its own %LOCALAPPDATA%, so nothing touches the real app data.
    fn sandbox(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("lw-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        std::env::set_var("LOCALAPPDATA", &root);
        root
    }

    fn repo() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    // The env var is process-wide, so the cases run one after another in a single test.
    #[test]
    fn imports() {
        let root = sandbox("imports");

        // A Lively zip from this repo.
        let zip = repo().join("wallpapers/clawd-dunes/Clawd-Dunes.zip");
        let r = import(&[zip]);
        let id = r[0].clone().expect("zip imports");
        let lib = Library::scan();
        let w = lib.get(&id).expect("listed after import");
        assert_eq!(w.title, "Clawd Dunes");
        assert!(matches!(&w.source, Source::Page(p) if p == "index.html"));
        assert!(!w.props.is_empty(), "keeps its settings");
        assert!(w.thumbnail.is_some());
        assert!(!w.host.contains(':') && w.host.len() < 64);

        // A picture becomes a picture wallpaper with fit settings.
        let pic = repo().join("wallpapers/claude-dunes/still-images/claude-dunes-night.png");
        let id = import(&[pic])[0].clone().expect("picture imports");
        let lib = Library::scan();
        let w = lib.get(&id).unwrap();
        assert_eq!(w.kind, "picture");
        assert_eq!(w.thumbnail.as_deref(), Some("wallpaper.png"));
        assert!(w.prop("fit").is_some() && w.prop("volume").is_none());
        let html = fs::read_to_string(w.dir.join("index.html")).unwrap();
        assert!(html.contains(r#"<img id="m" src="wallpaper.png""#));

        // A lone web page, titled from its <title>.
        let page = root.join("page.html");
        fs::write(&page, "<html><head><title> Neon Rain </title></head><body>hi</body></html>").unwrap();
        let id = import(&[page])[0].clone().expect("page imports");
        assert_eq!(Library::scan().get(&id).unwrap().title, "Neon Rain");

        // A folder with an index.html; then the same again gets its own folder.
        let folder = root.join("My Thing");
        fs::create_dir_all(folder.join("js")).unwrap();
        fs::write(folder.join("index.html"), "<script src=js/a.js></script>").unwrap();
        fs::write(folder.join("js/a.js"), "1").unwrap();
        let a = import(&[folder.clone()])[0].clone().expect("folder imports");
        let b = import(&[folder])[0].clone().expect("folder imports twice");
        assert_ne!(a, b);
        assert!(Library::scan().get(&a).unwrap().dir.join("js/a.js").is_file());

        // Things that aren't wallpapers say why, and leave nothing behind.
        let txt = root.join("notes.txt");
        fs::write(&txt, "x").unwrap();
        let before = fs::read_dir(user_dir()).unwrap().count();
        assert!(import(&[txt])[0].is_err());
        let empty = root.join("empty.zip");
        {
            let f = fs::File::create(&empty).unwrap();
            let mut z = zip::ZipWriter::new(f);
            z.start_file::<_, ()>("readme.txt", Default::default()).unwrap();
            std::io::Write::write_all(&mut z, b"nothing").unwrap();
            z.finish().unwrap();
        }
        assert!(import(&[empty])[0].as_ref().unwrap_err().contains("no wallpaper"));
        assert_eq!(fs::read_dir(user_dir()).unwrap().count(), before);

        // A starter wallpaper, then removing it.
        let id = create_starter().unwrap();
        let lib = Library::scan();
        let w = lib.get(&id).unwrap().clone();
        assert_eq!(w.props.len(), 4);
        delete(&w).unwrap();
        assert!(Library::scan().get(&id).is_none());

        // Built-ins unpack, list in order, and can't be deleted.
        prepare_builtins();
        let lib = Library::scan();
        let ids: Vec<&str> = lib.items.iter().filter(|w| w.builtin).map(|w| w.id.as_str()).collect();
        assert_eq!(&ids[..3], &["claude-dunes", "clawd-dunes", "clawd-desk-buddy"]);
        assert_eq!(lib.get("clawd-tunes").unwrap().helper, Helper::Ears);
        assert!(lib.get("clawd-tunes").unwrap().audio);
        assert!(delete(lib.get("claude-dunes").unwrap()).is_err());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn settings_values() {
        let slider = json!({ "type": "slider", "min": 0, "max": 300, "step": 10, "value": 100 });
        assert_eq!(coerce(&slider, &json!(450)), Some(json!(300)));
        assert_eq!(coerce(&slider, &json!("120")), Some(json!(120)));
        let dd = json!({ "type": "dropdown", "items": ["a", "b"], "value": 0 });
        assert_eq!(coerce(&dd, &json!(1)), Some(json!(1)));
        assert_eq!(coerce(&dd, &json!(5)), None);
        let cb = json!({ "type": "checkbox", "value": true });
        assert_eq!(coerce(&cb, &json!("yes")), None);
        let pic = json!({ "type": "folderDropdown", "folder": "pictures", "value": "a.jpg" });
        assert_eq!(dispatch_value(&pic, &json!("b.png")), json!(r"pictures\b.png"));
    }
}
