// Packs the wallpapers in ../wallpapers/*/source into the exe, so the app ships them with nothing to import,
// and compiles the icon and manifest into it.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

// Files a helper or a browser preview leaves behind in a source folder. They're never part of a wallpaper.
const SKIP: &[&str] = &["clawd-eyes-data.js", "clawd-tunes-data.js", "clawd-tunes-art.js", "desktop.ini", "thumbs.db"];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let wallpapers = manifest_dir.join("..").join("wallpapers");
    println!("cargo:rerun-if-changed={}", wallpapers.display());
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=res/app.manifest");
    println!("cargo:rerun-if-changed=res/app.ico");

    let mut ids: Vec<String> = Vec::new();
    if let Ok(rd) = fs::read_dir(&wallpapers) {
        for e in rd.flatten() {
            if e.path().join("source").join("LivelyInfo.json").is_file() {
                ids.push(e.file_name().to_string_lossy().into_owned());
            }
        }
    }
    ids.sort();

    let mut code = String::from("pub static BUILTIN_FILES: &[(&str, &str, &[u8])] = &[\n");
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut mix = |bytes: &[u8]| {
        for b in bytes {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    mix(env::var("CARGO_PKG_VERSION").unwrap().as_bytes());
    for id in &ids {
        let root = wallpapers.join(id).join("source");
        // Every file in a source folder changes what gets embedded, so watch each one.
        println!("cargo:rerun-if-changed={}", root.display());
        let mut files = Vec::new();
        collect(&root, &root, &mut files);
        files.sort();
        for rel in files {
            let abs = root.join(&rel);
            println!("cargo:rerun-if-changed={}", abs.display());
            let bytes = fs::read(&abs).unwrap();
            let rel_url = rel.replace('\\', "/");
            mix(id.as_bytes());
            mix(rel_url.as_bytes());
            mix(&bytes);
            code.push_str(&format!(
                "    ({:?}, {:?}, include_bytes!({:?})),\n",
                id,
                rel_url,
                abs.to_string_lossy()
            ));
        }
    }
    code.push_str("];\n");
    code.push_str(&format!("pub const BUILTIN_HASH: &str = \"{hash:016x}\";\n"));
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("builtin.rs");
    fs::write(out, code).unwrap();

    embed_resource::compile("res/app.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_lowercase();
        if p.is_dir() {
            collect(root, &p, out);
        } else if !SKIP.contains(&name.as_str()) && !name.ends_with(".tmp") {
            out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
        }
    }
}
