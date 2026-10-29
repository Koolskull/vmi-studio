//! Port of `vmi_studio/fonts.py`: the UI faces from the bgcardbuilder catalog.
//!
//! The kits in that repo are mostly WOFF. egui loads TrueType, so a WOFF is
//! unpacked into the cache the first time it is used. Google faces in the same
//! catalog are downloaded the first time they are chosen. The default face is
//! the system monospace: the house stack when the cache holds it, then Courier
//! New, else the mono face egui ships. The cache is the Python's, with the
//! Python's file names, so one unpack or download serves both programs.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use flate2::read::ZlibDecoder;
use regex::Regex;

/// The Python default id: the system monospace.
pub const DEFAULT_FACE: &str = "monospace";
/// The header groups faces in this order, then by name.
pub const CATEGORY_ORDER: [&str; 5] = ["monospace", "pixel", "sans-serif", "serif", "display"];
/// The house faces, best first, by catalog id. The window opens in the first one the cache holds.
pub const HOUSE_STACK: [&str; 2] = ["courier-pixel", "sometype-mono"];
/// The system face the house stack ends on, before egui's own mono.
pub const SYSTEM_MONO: &str = "Courier New";

const UA: &str = "Mozilla/5.0 (Windows NT 6.1; WOW64; rv:40.0) Gecko/20100101 Firefox/40.0";
/// Google serves TrueType to a browser it does not know. Asked when the first answer is WOFF2,
/// which the Python hands to fontTools and this crate cannot unpack.
const PLAIN_UA: &str = "vmi-studio";
const CSS_TIMEOUT: Duration = Duration::from_secs(30);
const FONT_TIMEOUT: Duration = Duration::from_secs(60);
/// A cached file this short is a failed write, not a font.
const MIN_FONT_BYTES: u64 = 64;

/// Where a face's bytes come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A file of the catalog repo, resolved to its full path: `.ttf`, `.otf`, `.woff`, or `.woff2`.
    Bundled(PathBuf),
    /// The Google Fonts CSS2 URL for the family. The font file URL is read out of that CSS.
    Url(String),
    /// A family the system has, like "Courier New". `monospace` is the system monospace.
    System(String),
}

/// One row of the font menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    /// The Python id, from the catalog's `fonts.ts`.
    pub id: String,
    /// The menu text, the catalog's `name`.
    pub label: String,
    /// `monospace`, `pixel`, `sans-serif`, `serif`, or `display`.
    pub category: String,
    /// The cached file name, `<id>.ttf`, like the Python writes it.
    pub file: String,
    pub source: Source,
}

impl Face {
    fn new(id: &str, label: &str, category: &str, source: Source) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            category: category.to_string(),
            file: format!("{id}.ttf"),
            source,
        }
    }

    fn default_face() -> Self {
        Self::new(
            DEFAULT_FACE,
            "Monospace",
            "monospace",
            Source::System(DEFAULT_FACE.to_string()),
        )
    }
}

/// The header's word for a category.
pub fn category_label(category: &str) -> &str {
    match category {
        "monospace" => "Monospace",
        "pixel" => "Pixel",
        "sans-serif" => "Sans",
        "serif" => "Serif",
        "display" => "Display",
        other => other,
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The optional font catalog: `BGCARDBUILDER`, else `~/Documents/work/Github/bgcardbuilder`
/// when it holds `src/data`. None means the menu is the system monospace only.
pub fn catalog_root() -> Option<PathBuf> {
    if let Some(env) = std::env::var_os("BGCARDBUILDER").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(env));
    }
    let home = home_dir()
        .join("Documents")
        .join("work")
        .join("Github")
        .join("bgcardbuilder");
    home.join("src").join("data").is_dir().then_some(home)
}

/// `~/.cache/vmi-studio/fonts`, shared with the Python app.
pub fn cache_dir() -> PathBuf {
    cache_dir_in(&home_dir())
}

fn cache_dir_in(home: &Path) -> PathBuf {
    home.join(".cache").join("vmi-studio").join("fonts")
}

/// Every face, Python order: the system monospace, then the catalog's entries as its `fonts.ts` lists them.
pub fn catalog() -> Vec<Face> {
    static CATALOG: OnceLock<Vec<Face>> = OnceLock::new();
    CATALOG
        .get_or_init(|| catalog_in(catalog_root().as_deref()))
        .clone()
}

/// The catalog read from one root. None is the monospace-only menu.
pub fn catalog_in(root: Option<&Path>) -> Vec<Face> {
    let mut faces = vec![Face::default_face()];
    let Some(root) = root else {
        return faces;
    };
    let files = fs::read_to_string(root.join("src").join("index.css"))
        .map(|css| font_files(&css, root))
        .unwrap_or_default();
    let Ok(text) = fs::read_to_string(root.join("src").join("data").join("fonts.ts")) else {
        return faces;
    };
    // Family is single-quoted in the Python pattern. A family that itself
    // contains a quote is written with double quotes in fonts.ts, so both count.
    let entry = Regex::new(
        r#"(?s)id:\s*'(?P<id>[^']+)'\s*,\s*name:\s*'(?P<name>[^']*)'\s*,\s*family:\s*(?:'(?P<single>[^']*)'|"(?P<double>[^"]*)")\s*,\s*category:\s*'(?P<category>[^']+)'"#,
    )
    .expect("entry pattern");
    let mut seen = vec![DEFAULT_FACE.to_string()];
    for found in entry.captures_iter(&text) {
        let id = &found["id"];
        if seen.iter().any(|known| known == id) {
            continue;
        }
        seen.push(id.to_string());
        let family = found.name("single").or_else(|| found.name("double")).map(|value| value.as_str()).unwrap_or("");
        let name = css_name(family);
        let source = match files.iter().find(|(family, _)| *family == name) {
            Some((_, path)) => Source::Bundled(path.clone()),
            None => Source::Url(google_css_url(&name)),
        };
        faces.push(Face::new(id, &found["name"], &found["category"], source));
    }
    faces
}

pub fn face(id: &str) -> Option<Face> {
    catalog().into_iter().find(|face| face.id == id)
}

/// Where the cache keeps this face, whether or not it is there yet.
pub fn cached_path(face: &Face) -> PathBuf {
    cache_dir().join(&face.file)
}

/// Make the face available in the cache: unpack a WOFF with zlib, use a bundled TTF where it
/// lies, download a Google face. Returns the TTF/OTF path, or the Python's error text.
pub fn ensure(face: &Face) -> Result<PathBuf, String> {
    resolve(face, &cache_dir(), true)
}

/// The face's path when no download is needed: the Python's `preview_family`.
pub fn preview(face: &Face) -> Option<PathBuf> {
    resolve(face, &cache_dir(), false).ok()
}

/// The bytes of the best house face the cache holds: courier-pixel, then sometype-mono, else
/// None and the caller falls back to the system or egui mono. The name is the stack id.
pub fn house_bytes() -> Option<(String, Vec<u8>)> {
    let (id, path) = house_path_in(&cache_dir())?;
    let bytes = fs::read(path).ok().filter(|bytes| looks_like_font(bytes))?;
    Some((id, bytes))
}

fn resolve(face: &Face, cache: &Path, fetch: bool) -> Result<PathBuf, String> {
    let path = match &face.source {
        Source::System(name) => {
            system_path(name, cache).ok_or_else(|| format!("no system face {name}"))?
        }
        Source::Bundled(src) => local_ttf(face, src, cache)?,
        Source::Url(css_url) => {
            let dest = cache.join(&face.file);
            if is_font_file(&dest) {
                dest
            } else {
                if !fetch {
                    return Err("not loaded".to_string());
                }
                fetch_google(css_url, &dest)?;
                dest
            }
        }
    };
    remember(face, path, cache)
}

/// The Python's `_remember`: a file the loader rejects leaves the cache and fails the face.
fn remember(face: &Face, path: PathBuf, cache: &Path) -> Result<PathBuf, String> {
    let bytes = fs::read(&path).map_err(|err| err.to_string())?;
    if looks_like_font(&bytes) {
        return Ok(path);
    }
    if path.parent() == Some(cache) {
        let _ = fs::remove_file(&path);
    }
    Err(format!("Could not load {}.", face.label))
}

/// The system monospace is the house stack from the cache, then Courier New. Any other
/// family is looked for in the system font folders by file name.
fn system_path(name: &str, cache: &Path) -> Option<PathBuf> {
    if name == DEFAULT_FACE {
        return house_path_in(cache)
            .map(|(_, path)| path)
            .or_else(|| system_face_path(SYSTEM_MONO));
    }
    system_face_path(name)
}

fn house_path_in(cache: &Path) -> Option<(String, PathBuf)> {
    let entries: Vec<PathBuf> = fs::read_dir(cache)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    HOUSE_STACK.iter().find_map(|id| {
        let exact = ["ttf", "otf"]
            .iter()
            .map(|ext| cache.join(format!("{id}.{ext}")))
            .find(|path| is_font_file(path));
        // Older caches hold the face under another name; the parts of the id still name it.
        let parts: Vec<&str> = id.split('-').collect();
        let loose = || {
            entries
                .iter()
                .filter(|path| is_font_file(path))
                .find(|path| {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    parts.iter().all(|part| name.contains(part))
                })
                .cloned()
        };
        exact.or_else(loose).map(|path| (id.to_string(), path))
    })
}

fn is_font_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    (ext == "ttf" || ext == "otf")
        && fs::metadata(path)
            .map(|meta| meta.is_file() && meta.len() > MIN_FONT_BYTES)
            .unwrap_or(false)
}

/// The folders the system keeps its faces in, whichever of them exist here.
fn system_font_dirs() -> Vec<PathBuf> {
    let home = home_dir();
    let mut dirs = vec![
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/System/Library/Fonts/Supplemental"),
        PathBuf::from("/Library/Fonts"),
        home.join("Library").join("Fonts"),
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
        home.join(".fonts"),
        home.join(".local").join("share").join("fonts"),
    ];
    if let Some(windir) = std::env::var_os("WINDIR") {
        dirs.push(PathBuf::from(windir).join("Fonts"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(
            PathBuf::from(local)
                .join("Microsoft")
                .join("Windows")
                .join("Fonts"),
        );
    }
    dirs.retain(|dir| dir.is_dir());
    dirs
}

fn system_face_path(family: &str) -> Option<PathBuf> {
    system_face_in(&system_font_dirs(), family)
}

/// A regular face of `family` under `dirs`: the file is named after the family, with or
/// without "Regular", spaces, dashes, and underscores; Courier New is also Windows's `cour.ttf`.
fn system_face_in(dirs: &[PathBuf], family: &str) -> Option<PathBuf> {
    let wanted = squash(family);
    let mut stems = vec![wanted.clone(), format!("{wanted}regular")];
    if wanted == "couriernew" {
        stems.push("cour".to_string());
    }
    let mut found = Vec::new();
    for dir in dirs {
        walk_fonts(dir, 0, &mut found);
    }
    found.sort();
    found.into_iter().find(|path| {
        let stem = path
            .file_stem()
            .map(|stem| squash(&stem.to_string_lossy()))
            .unwrap_or_default();
        stems.iter().any(|candidate| *candidate == stem)
    })
}

fn squash(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn walk_fonts(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_fonts(&path, depth + 1, found);
        } else if is_font_file(&path) {
            found.push(path);
        }
    }
}

/// The Python's `_local_ttf`: a TTF or OTF is used where it lies, a WOFF is unpacked into the
/// cache once per change of the source. WOFF2 needs brotli, which the Python gets from fontTools.
fn local_ttf(face: &Face, src: &Path, cache: &Path) -> Result<PathBuf, String> {
    let ext = src
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "ttf" || ext == "otf" {
        return Ok(src.to_path_buf());
    }
    let dest = cache.join(&face.file);
    if is_font_file(&dest) && modified(&dest) >= modified(src) {
        return Ok(dest);
    }
    fs::create_dir_all(cache).map_err(|err| err.to_string())?;
    match ext.as_str() {
        "woff" => {
            let data = fs::read(src).map_err(|err| err.to_string())?;
            write_font(&dest, &woff_to_sfnt(&data)?)?;
            Ok(dest)
        }
        "woff2" => Err("woff2 needs fontTools".to_string()),
        _ => Err("unsupported font file".to_string()),
    }
}

fn modified(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// The file lands whole: a reader on the other program never sees a half.
fn write_font(dest: &Path, data: &[u8]) -> Result<(), String> {
    let part = dest.with_extension("ttf.part");
    fs::write(&part, data).map_err(|err| err.to_string())?;
    fs::rename(&part, dest).map_err(|err| err.to_string())
}

/// The Python's `_fetch_google`: read the family's CSS, take the latin file, keep it as TrueType.
fn fetch_google(css_url: &str, dest: &Path) -> Result<(), String> {
    let (mut kind, mut raw) = google_file(css_url, UA)?;
    if kind == *b"wOF2" {
        let again = google_file(css_url, PLAIN_UA)?;
        kind = again.0;
        raw = again.1;
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    match &kind {
        b"wOF2" => Err("woff2 needs fontTools".to_string()),
        b"wOFF" => write_font(dest, &woff_to_sfnt(&raw)?),
        b"\x00\x01\x00\x00" | b"OTTO" => write_font(dest, &raw),
        _ => Err("not a font".to_string()),
    }
}

fn google_file(css_url: &str, agent: &str) -> Result<([u8; 4], Vec<u8>), String> {
    let css = String::from_utf8_lossy(&read_url(css_url, agent, CSS_TIMEOUT)?).into_owned();
    let url = pick_font_url(&css)?;
    if host_of(&url) != "fonts.gstatic.com" {
        return Err("unexpected font host".to_string());
    }
    let raw = read_url(&url, agent, FONT_TIMEOUT)?;
    if raw.len() < 4 {
        return Err("not a font".to_string());
    }
    let mut kind = [0u8; 4];
    kind.copy_from_slice(&raw[..4]);
    Ok((kind, raw))
}

fn read_url(url: &str, agent: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .set("User-Agent", agent)
        .timeout(timeout)
        .call()
        .map_err(|err| err.to_string())?;
    let mut data = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut data)
        .map_err(|err| err.to_string())?;
    Ok(data)
}

fn host_of(url: &str) -> &str {
    let rest = url.find("://").map(|at| &url[at + 3..]).unwrap_or("");
    rest.split('/').next().unwrap_or("")
}

/// The Python's `_pick_font_url`: the first block without a unicode range, else the latin
/// block, else the first block.
fn pick_font_url(css: &str) -> Result<String, String> {
    let url_in = Regex::new(r"url\(\s*([^)\s]+)\s*\)").expect("url pattern");
    let mut fallback = None;
    let mut latin = None;
    for block in face_blocks(css) {
        let Some(found) = url_in.captures(&block) else {
            continue;
        };
        let url = found[1]
            .trim_matches(|ch| ch == '\'' || ch == '"')
            .to_string();
        if !block.contains("unicode-range") {
            return Ok(url);
        }
        if fallback.is_none() {
            fallback = Some(url.clone());
        }
        let low = block.to_lowercase();
        if low.contains("/* latin */") || low.contains("u+0000-00ff") {
            latin = Some(url);
        }
    }
    latin.or(fallback).ok_or_else(|| "no font file".to_string())
}

fn face_blocks(css: &str) -> Vec<String> {
    let block = Regex::new(r"(?i)@font-face\s*\{([^}]+)\}").expect("face pattern");
    block
        .captures_iter(css)
        .map(|found| found[1].to_string())
        .collect()
}

/// The Google Fonts CSS2 URL the Python asks for a family.
pub fn google_css_url(family: &str) -> String {
    format!(
        "https://fonts.googleapis.com/css2?family={}&display=swap",
        quote_plus(family)
    )
}

/// Python's `urllib.parse.quote_plus`.
fn quote_plus(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b' ' => out.push('+'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The first family of a CSS font-family list, unquoted.
fn css_name(family: &str) -> String {
    let head = family.split(',').next().unwrap_or("").trim();
    let chars: Vec<char> = head.chars().collect();
    if chars.len() >= 2 && chars[0] == chars[chars.len() - 1] && matches!(chars[0], '"' | '\'') {
        return chars[1..chars.len() - 1].iter().collect();
    }
    head.to_string()
}

/// The Python's `_font_files`: for each `@font-face` family in the catalog's CSS, the best local
/// file it names (TTF over OTF over WOFF over WOFF2) that exists under the root.
fn font_files(css: &str, root: &Path) -> Vec<(String, PathBuf)> {
    let family_in = Regex::new(r"font-family:\s*([^;]+);").expect("family pattern");
    let url_in = Regex::new(r#"url\(\s*['"]?([^)'"]+)['"]?\s*\)"#).expect("url pattern");
    let rank = |ext: &str| match ext {
        "ttf" => Some(0),
        "otf" => Some(1),
        "woff" => Some(2),
        "woff2" => Some(3),
        _ => None,
    };
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for block in face_blocks(css) {
        let Some(family) = family_in.captures(&block) else {
            continue;
        };
        let name = family[1]
            .trim()
            .trim_matches(|ch| ch == '\'' || ch == '"')
            .to_string();
        if found.iter().any(|(known, _)| *known == name) {
            continue;
        }
        let mut best: Option<(usize, PathBuf)> = None;
        for url in url_in.captures_iter(&block) {
            let path = url[1].split('?').next().unwrap_or("");
            let ext = Path::new(path)
                .extension()
                .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let Some(score) = rank(&ext) else {
                continue;
            };
            if best
                .as_ref()
                .map(|(held, _)| score >= *held)
                .unwrap_or(false)
            {
                continue;
            }
            let full = match path.strip_prefix('/') {
                Some(public) => root.join("public").join(public.trim_start_matches('/')),
                None => normalize(&root.join(path)),
            };
            if full.is_file() {
                best = Some((score, full));
            }
        }
        if let Some((_, path)) = best {
            found.push((name, path));
        }
    }
    found
}

/// Python's `os.path.normpath`, without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Unpack a WOFF (not WOFF2) into an SFNT the font loader can open.
pub fn woff_to_sfnt(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 44 || &data[..4] != b"wOFF" {
        return Err("not a woff".to_string());
    }
    let flavor = be_u32(data, 4);
    let num_tables = be_u16(data, 12) as usize;
    let mut entries: Vec<([u8; 4], u32, Vec<u8>)> = Vec::with_capacity(num_tables);
    let mut offset = 44;
    for _ in 0..num_tables {
        if offset + 20 > data.len() {
            return Err("bad table".to_string());
        }
        let mut tag = [0u8; 4];
        tag.copy_from_slice(&data[offset..offset + 4]);
        let table_off = be_u32(data, offset + 4) as usize;
        let comp = be_u32(data, offset + 8) as usize;
        let orig = be_u32(data, offset + 12) as usize;
        let checksum = be_u32(data, offset + 16);
        offset += 20;
        let name = String::from_utf8_lossy(&tag).into_owned();
        let blob = data
            .get(table_off..table_off.saturating_add(comp))
            .ok_or_else(|| format!("bad table {name:?}"))?;
        let blob = if comp != orig {
            let mut out = Vec::with_capacity(orig);
            ZlibDecoder::new(blob)
                .read_to_end(&mut out)
                .map_err(|_| format!("bad table {name:?}"))?;
            out
        } else {
            blob.to_vec()
        };
        if blob.len() != orig {
            return Err(format!("bad table {name:?}"));
        }
        entries.push((tag, checksum, blob));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let count = entries.len();
    let mut search = 1usize;
    let mut selector = 0u16;
    while search * 2 <= count {
        search *= 2;
        selector += 1;
    }
    let mut out = Vec::new();
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&(count as u16).to_be_bytes());
    out.extend_from_slice(&((search * 16) as u16).to_be_bytes());
    out.extend_from_slice(&selector.to_be_bytes());
    out.extend_from_slice(&((count * 16 - search * 16) as u16).to_be_bytes());
    let mut body = Vec::new();
    let mut cursor = 12 + 16 * count;
    for (tag, checksum, blob) in &entries {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&(cursor as u32).to_be_bytes());
        out.extend_from_slice(&(blob.len() as u32).to_be_bytes());
        body.extend_from_slice(blob);
        let pad = (4 - blob.len() % 4) % 4;
        body.extend(std::iter::repeat(0u8).take(pad));
        cursor += blob.len() + pad;
    }
    out.extend_from_slice(&body);
    Ok(out)
}

/// An SFNT the font loader will open: a TrueType or CFF header, a table directory inside the
/// file, and the `head`, `hhea`, and `maxp` tables the parser insists on. egui panics on less.
pub fn looks_like_font(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }
    if !matches!(&data[..4], b"\x00\x01\x00\x00" | b"OTTO" | b"true") {
        return false;
    }
    let count = be_u16(data, 4) as usize;
    if count == 0 || 12 + 16 * count > data.len() {
        return false;
    }
    let mut has = [false; 3];
    for index in 0..count {
        let at = 12 + 16 * index;
        let tag = &data[at..at + 4];
        let offset = be_u32(data, at + 8) as usize;
        let length = be_u32(data, at + 12) as usize;
        if offset.saturating_add(length) > data.len() {
            return false;
        }
        match tag {
            b"head" => has[0] = true,
            b"hhea" => has[1] = true,
            b"maxp" => has[2] = true,
            _ => {}
        }
    }
    has.iter().all(|found| *found)
}

fn be_u16(data: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([data[at], data[at + 1]])
}

fn be_u32(data: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("vmi-fonts-{name}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The tables a loader insists on, each big enough to tell apart.
    fn tables() -> Vec<([u8; 4], Vec<u8>)> {
        vec![
            (*b"head", vec![1u8; 54]),
            (*b"hhea", vec![2u8; 36]),
            (*b"maxp", vec![3u8; 6]),
            (*b"cmap", (0..=200u8).collect()),
            (*b"glyf", vec![4u8; 13]),
        ]
    }

    fn sfnt(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut sorted: Vec<&([u8; 4], Vec<u8>)> = tables.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let count = sorted.len();
        let mut out = vec![0, 1, 0, 0];
        out.extend_from_slice(&(count as u16).to_be_bytes());
        out.extend_from_slice(&[0u8; 6]);
        let mut body = Vec::new();
        let mut cursor = 12 + 16 * count;
        for (tag, blob) in sorted {
            out.extend_from_slice(tag);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&(cursor as u32).to_be_bytes());
            out.extend_from_slice(&(blob.len() as u32).to_be_bytes());
            body.extend_from_slice(blob);
            let pad = (4 - blob.len() % 4) % 4;
            body.extend(std::iter::repeat(0u8).take(pad));
            cursor += blob.len() + pad;
        }
        out.extend_from_slice(&body);
        out
    }

    /// A WOFF of `tables` in the given order; every table but `stored` is zlib-compressed.
    fn woff(tables: &[([u8; 4], Vec<u8>)], stored: &[u8; 4]) -> Vec<u8> {
        let count = tables.len();
        let mut out = vec![b'w', b'O', b'F', b'F', 0, 1, 0, 0, 0, 0, 0, 0];
        out.extend_from_slice(&(count as u16).to_be_bytes());
        out.extend(std::iter::repeat(0u8).take(44 - out.len()));
        let mut body = Vec::new();
        let mut cursor = 44 + 20 * count;
        for (tag, blob) in tables {
            let packed = if tag == stored {
                blob.clone()
            } else {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(blob).unwrap();
                encoder.finish().unwrap()
            };
            out.extend_from_slice(tag);
            out.extend_from_slice(&(cursor as u32).to_be_bytes());
            out.extend_from_slice(&(packed.len() as u32).to_be_bytes());
            out.extend_from_slice(&(blob.len() as u32).to_be_bytes());
            out.extend_from_slice(&0xC0DEu32.to_be_bytes());
            let pad = (4 - packed.len() % 4) % 4;
            body.extend_from_slice(&packed);
            body.extend(std::iter::repeat(0u8).take(pad));
            cursor += packed.len() + pad;
        }
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn the_catalog_without_a_root_is_the_system_monospace() {
        let faces = catalog_in(None);
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].id, DEFAULT_FACE);
        assert_eq!(faces[0].label, "Monospace");
        assert_eq!(faces[0].category, "monospace");
        assert_eq!(faces[0].file, "monospace.ttf");
        assert_eq!(faces[0].source, Source::System("monospace".to_string()));
        assert_eq!(CATEGORY_ORDER[0], "monospace");
        assert_eq!(category_label("sans-serif"), "Sans");
        assert_eq!(category_label("display"), "Display");
        let home = Path::new("/home/someone");
        assert_eq!(
            cache_dir_in(home),
            home.join(".cache").join("vmi-studio").join("fonts")
        );
        assert!(cache_dir().ends_with(Path::new(".cache").join("vmi-studio").join("fonts")));
    }

    #[test]
    fn the_catalog_reads_the_repo_like_the_python() {
        let root = scratch("catalog");
        fs::create_dir_all(root.join("src").join("data")).unwrap();
        fs::create_dir_all(root.join("public").join("fonts")).unwrap();
        fs::create_dir_all(root.join("kits")).unwrap();
        fs::write(
            root.join("public").join("fonts").join("courier-pixel.woff"),
            b"woff",
        )
        .unwrap();
        fs::write(root.join("kits").join("Zed.woff"), b"woff").unwrap();
        fs::write(root.join("kits").join("Zed.ttf"), b"ttf").unwrap();
        fs::write(
            root.join("src").join("index.css"),
            concat!(
                "@font-face { font-family: 'Courier Pixel'; src: url('/fonts/courier-pixel.woff?v=3') format('woff'); }\n",
                "@FONT-FACE { font-family: \"Zed\"; src: url(./kits/Zed.woff) format('woff'), url(\"kits/../kits/Zed.ttf\"); }\n",
                "@font-face { font-family: 'Gone'; src: url('/fonts/gone.woff'); }\n",
            ),
        )
        .unwrap();
        fs::write(
            root.join("src").join("data").join("fonts.ts"),
            concat!(
                "export const FONTS = [\n",
                "  { id: 'courier-pixel', name: 'Courier Pixel', family: \"'Courier Pixel', monospace\", category: 'pixel' },\n",
                "  { id: 'sometype-mono', name: 'Sometype Mono',\n    family: 'Sometype Mono, monospace', category: 'monospace' },\n",
                "  { id: 'zed', name: 'Zed Display', family: 'Zed', category: 'display' },\n",
                "  { id: 'courier-pixel', name: 'Twice', family: 'Courier Pixel', category: 'pixel' },\n",
                "  { id: 'monospace', name: 'Shadow', family: 'monospace', category: 'monospace' },\n",
                "];\n",
            ),
        )
        .unwrap();
        let faces = catalog_in(Some(&root));
        let ids: Vec<&str> = faces.iter().map(|face| face.id.as_str()).collect();
        assert_eq!(ids, ["monospace", "courier-pixel", "sometype-mono", "zed"]);
        assert_eq!(faces[1].label, "Courier Pixel");
        assert_eq!(faces[1].category, "pixel");
        assert_eq!(faces[1].file, "courier-pixel.ttf");
        assert_eq!(
            faces[1].source,
            Source::Bundled(root.join("public").join("fonts").join("courier-pixel.woff"))
        );
        assert_eq!(
            faces[2].source,
            Source::Url(
                "https://fonts.googleapis.com/css2?family=Sometype+Mono&display=swap".to_string()
            )
        );
        assert_eq!(faces[3].label, "Zed Display");
        assert_eq!(
            faces[3].source,
            Source::Bundled(root.join("kits").join("Zed.ttf"))
        );
        assert_eq!(quote_plus("Noto Sans JP & Co."), "Noto+Sans+JP+%26+Co.");
        assert_eq!(css_name("\"Press Start 2P\", cursive"), "Press Start 2P");
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_woff_unpacks_into_a_sorted_padded_sfnt() {
        let tables = tables();
        let packed = woff(&tables, b"maxp");
        let sfnt_out = woff_to_sfnt(&packed).unwrap();
        assert_eq!(&sfnt_out[..4], &[0, 1, 0, 0]);
        assert_eq!(be_u16(&sfnt_out, 4), 5);
        // 5 tables: search range 4 * 16, selector 2, range shift 80 - 64.
        assert_eq!(be_u16(&sfnt_out, 6), 64);
        assert_eq!(be_u16(&sfnt_out, 8), 2);
        assert_eq!(be_u16(&sfnt_out, 10), 16);
        let mut tags = Vec::new();
        for index in 0..5 {
            let at = 12 + 16 * index;
            let tag = &sfnt_out[at..at + 4];
            assert_eq!(be_u32(&sfnt_out, at + 4), 0xC0DE);
            let offset = be_u32(&sfnt_out, at + 8) as usize;
            let length = be_u32(&sfnt_out, at + 12) as usize;
            assert_eq!(offset % 4, 0);
            let (_, original) = tables.iter().find(|(known, _)| known == tag).unwrap();
            assert_eq!(&sfnt_out[offset..offset + length], &original[..]);
            tags.push(String::from_utf8_lossy(tag).into_owned());
        }
        assert_eq!(tags, ["cmap", "glyf", "head", "hhea", "maxp"]);
        assert!(looks_like_font(&sfnt_out));
        assert_eq!(woff_to_sfnt(b"wOFFshort").unwrap_err(), "not a woff");
        assert_eq!(woff_to_sfnt(&sfnt_out).unwrap_err(), "not a woff");
        let mut torn = packed.clone();
        torn.truncate(44 + 20 * 5 + 8);
        assert!(torn.len() > 44 + 20 * 5);
        assert!(woff_to_sfnt(&torn).unwrap_err().starts_with("bad table"));
        assert!(!looks_like_font(b"not a font at all, really not"));
        assert!(!looks_like_font(&sfnt(&tables[3..])));
    }

    #[test]
    fn a_bundled_woff_lands_in_the_cache_once_and_a_ttf_is_used_in_place() {
        let root = scratch("bundled");
        let cache = root.join("cache");
        let src = root.join("pixel.woff");
        fs::write(&src, woff(&tables(), b"glyf")).unwrap();
        let face = Face::new(
            "courier-pixel",
            "Courier Pixel",
            "pixel",
            Source::Bundled(src.clone()),
        );
        let path = resolve(&face, &cache, false).unwrap();
        assert_eq!(path, cache.join("courier-pixel.ttf"));
        assert!(looks_like_font(&fs::read(&path).unwrap()));
        let first = modified(&path);
        assert_eq!(resolve(&face, &cache, false).unwrap(), path);
        assert_eq!(modified(&path), first);
        let ttf = root.join("Zed.ttf");
        fs::write(&ttf, sfnt(&tables())).unwrap();
        let zed = Face::new("zed", "Zed", "display", Source::Bundled(ttf.clone()));
        assert_eq!(resolve(&zed, &cache, true).unwrap(), ttf);
        let junk = root.join("junk.ttf");
        fs::write(&junk, vec![7u8; 100]).unwrap();
        let bad = Face::new("junk", "Junk", "display", Source::Bundled(junk));
        assert_eq!(
            resolve(&bad, &cache, true).unwrap_err(),
            "Could not load Junk."
        );
        let two = Face::new(
            "two",
            "Two",
            "display",
            Source::Bundled(root.join("two.woff2")),
        );
        assert_eq!(
            resolve(&two, &cache, true).unwrap_err(),
            "woff2 needs fontTools"
        );
        let odd = Face::new(
            "odd",
            "Odd",
            "display",
            Source::Bundled(root.join("odd.svg")),
        );
        assert_eq!(
            resolve(&odd, &cache, true).unwrap_err(),
            "unsupported font file"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_google_face_reads_from_the_cache_and_never_fetches_in_a_preview() {
        let root = scratch("google");
        let cache = root.join("cache");
        let face = Face::new(
            "sometype-mono",
            "Sometype Mono",
            "monospace",
            Source::Url(google_css_url("Sometype Mono")),
        );
        assert_eq!(resolve(&face, &cache, false).unwrap_err(), "not loaded");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("sometype-mono.ttf"), sfnt(&tables())).unwrap();
        assert_eq!(
            resolve(&face, &cache, false).unwrap(),
            cache.join("sometype-mono.ttf")
        );
        assert_eq!(
            resolve(&face, &cache, true).unwrap(),
            cache.join("sometype-mono.ttf")
        );
        // A torn download leaves the cache and reads as not loaded again.
        fs::write(cache.join("sometype-mono.ttf"), vec![1u8; 300]).unwrap();
        assert_eq!(
            resolve(&face, &cache, false).unwrap_err(),
            "Could not load Sometype Mono."
        );
        assert!(!cache.join("sometype-mono.ttf").exists());
        assert_eq!(
            host_of("https://fonts.gstatic.com/s/x/v1/a.ttf"),
            "fonts.gstatic.com"
        );
        assert_eq!(host_of("nonsense"), "");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_font_url_comes_from_the_latin_block_or_the_first_plain_one() {
        let plain = "@font-face { font-family: 'A'; src: url(https://fonts.gstatic.com/s/a/one.ttf) format('truetype'); }";
        assert_eq!(
            pick_font_url(plain).unwrap(),
            "https://fonts.gstatic.com/s/a/one.ttf"
        );
        let ranged = concat!(
            "/* cyrillic */\n@font-face { font-family: 'A'; src: url(https://x/cyr.woff2) format('woff2'); unicode-range: U+0400-045F; }\n",
            "/* latin */\n@font-face { font-family: 'A'; src: url('https://x/lat.woff2') format('woff2'); unicode-range: U+0000-00FF; }\n",
        );
        assert_eq!(pick_font_url(ranged).unwrap(), "https://x/lat.woff2");
        let only = "@font-face { font-family: 'A'; src: url(https://x/greek.woff2); unicode-range: U+0370-03FF; }";
        assert_eq!(pick_font_url(only).unwrap(), "https://x/greek.woff2");
        assert_eq!(
            pick_font_url("body { color: red; }").unwrap_err(),
            "no font file"
        );
    }

    #[test]
    fn the_house_stack_prefers_courier_pixel_then_sometype_mono_then_the_system() {
        let root = scratch("house");
        let cache = root.join("cache");
        fs::create_dir_all(&cache).unwrap();
        assert_eq!(house_path_in(&cache), None);
        fs::write(cache.join("SometypeMono-Regular.ttf"), sfnt(&tables())).unwrap();
        assert_eq!(
            house_path_in(&cache),
            Some((
                "sometype-mono".to_string(),
                cache.join("SometypeMono-Regular.ttf")
            ))
        );
        fs::write(cache.join("sometype-mono.ttf"), sfnt(&tables())).unwrap();
        assert_eq!(
            house_path_in(&cache),
            Some(("sometype-mono".to_string(), cache.join("sometype-mono.ttf")))
        );
        fs::write(cache.join("courier-pixel.ttf"), sfnt(&tables())).unwrap();
        assert_eq!(
            house_path_in(&cache),
            Some(("courier-pixel".to_string(), cache.join("courier-pixel.ttf")))
        );
        // Too short to be a font: not a house face.
        fs::write(cache.join("courier-pixel.ttf"), b"tiny").unwrap();
        assert_eq!(house_path_in(&cache).unwrap().0, "sometype-mono");
        let system = root.join("system");
        fs::create_dir_all(system.join("deep").join("er")).unwrap();
        assert_eq!(system_face_in(&[system.clone()], "Courier New"), None);
        fs::write(
            system.join("deep").join("er").join("cour.ttf"),
            sfnt(&tables()),
        )
        .unwrap();
        assert_eq!(
            system_face_in(&[system.clone()], "Courier New"),
            Some(system.join("deep").join("er").join("cour.ttf"))
        );
        fs::write(system.join("Courier New.ttf"), sfnt(&tables())).unwrap();
        assert_eq!(
            system_face_in(&[system.clone()], "Courier New"),
            Some(system.join("Courier New.ttf"))
        );
        fs::write(system.join("SometypeMono-Regular.otf"), sfnt(&tables())).unwrap();
        assert_eq!(
            system_face_in(&[system.clone()], "Sometype Mono"),
            Some(system.join("SometypeMono-Regular.otf"))
        );
        assert_eq!(
            system_face_in(&[system.clone()], "Sometype Mono Bold"),
            None
        );
        let _ = fs::remove_dir_all(&root);
    }
}
