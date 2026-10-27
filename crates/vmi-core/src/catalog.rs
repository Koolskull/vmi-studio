//! The launcher book Python keeps at `~/.cache/vmi-studio/launcher.json`.
//! Projects, recent drawings, and the folder the browser opens on.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const DRAWING_EXTS: &[&str] = &["clip", "psd", "psb", "kra", "xcf", "vmib"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookFile {
    pub path: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookList {
    pub title: String,
    pub files: Vec<BookFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Book {
    pub recent: Vec<BookFile>,
    pub projects: Vec<BookList>,
    pub browse: String,
}

#[derive(Deserialize)]
struct RawBook {
    #[serde(default)]
    recent: Vec<RawFile>,
    #[serde(default)]
    lists: Vec<RawList>,
    #[serde(default)]
    browse: String,
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    path: String,
    #[serde(default)]
    label: String,
}

#[derive(Deserialize)]
struct RawList {
    #[serde(default)]
    title: String,
    #[serde(default)]
    style: String,
    #[serde(default)]
    items: Vec<RawFile>,
}

pub fn book_path() -> PathBuf {
    home().join(".cache/vmi-studio/launcher.json")
}

pub fn load_book(path: &Path) -> Book {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Book::default();
    };
    let Ok(raw) = serde_json::from_str::<RawBook>(&text) else {
        return Book::default();
    };
    Book {
        recent: raw.recent.iter().filter_map(file_of).collect(),
        projects: raw
            .lists
            .iter()
            .filter(|list| list.style == "project" || list.style.is_empty())
            .map(|list| BookList {
                title: if list.title.is_empty() { "Project".into() } else { list.title.clone() },
                files: list.items.iter().filter_map(file_of).collect(),
            })
            .collect(),
        browse: raw.browse,
    }
}

fn file_of(raw: &RawFile) -> Option<BookFile> {
    if raw.path.is_empty() {
        return None;
    }
    let label = if raw.label.is_empty() {
        Path::new(&raw.path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| raw.path.clone())
    } else {
        raw.label.clone()
    };
    Some(BookFile { path: raw.path.clone(), label })
}

pub fn is_drawing(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| DRAWING_EXTS.iter().any(|known| ext.eq_ignore_ascii_case(known)))
        .unwrap_or(false)
}

/// Saved browse folder, else `~/Documents/work`, else home.
pub fn default_browse(book: &Book) -> PathBuf {
    let saved = PathBuf::from(&book.browse);
    if !book.browse.is_empty() && saved.is_dir() {
        return saved;
    }
    let work = home().join("Documents/work");
    if work.is_dir() {
        return work;
    }
    home()
}

pub fn places() -> Vec<(&'static str, PathBuf)> {
    let home = home();
    let mut rows = vec![("Home", home.clone())];
    for (label, path) in [
        ("Work", home.join("Documents/work")),
        ("Documents", home.join("Documents")),
        ("Desktop", home.join("Desktop")),
        ("Downloads", home.join("Downloads")),
    ] {
        if path.is_dir() && path != home {
            rows.push((label, path));
        }
    }
    rows.push(("Computer", PathBuf::from("/")));
    rows
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_book_keeps_projects_and_recent_drawings() {
        let raw = r#"{
            "recent": [{"path": "/tmp/a.clip", "label": ""}],
            "lists": [
                {"title": "Beetle", "style": "project", "items": [{"path": "/tmp/b.kra", "label": "Bridge"}]},
                {"title": "notes", "style": "task", "items": [{"path": "/tmp/no.clip", "label": "skip"}]}
            ],
            "browse": "/tmp"
        }"#;
        let book: Book = {
            let raw: RawBook = serde_json::from_str(raw).unwrap();
            Book {
                recent: raw.recent.iter().filter_map(file_of).collect(),
                projects: raw
                    .lists
                    .iter()
                    .filter(|list| list.style == "project" || list.style.is_empty())
                    .map(|list| BookList {
                        title: list.title.clone(),
                        files: list.items.iter().filter_map(file_of).collect(),
                    })
                    .collect(),
                browse: raw.browse,
            }
        };
        assert_eq!(book.recent[0].label, "a.clip");
        assert_eq!(book.projects.len(), 1);
        assert_eq!(book.projects[0].files[0].label, "Bridge");
        assert!(is_drawing(Path::new("x.VMIB")));
        assert!(!is_drawing(Path::new("x.png")));
    }
}
