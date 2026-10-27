//! One drawing per tab, one export folder each. A port of `vmi_studio/tabs.py`.

use std::path::{Path, PathBuf};

use crate::naming::{folder_name, safe_folder};

/// The folder a scene is written into, from the drawing's file name.
pub fn scene_dirname(file_name: &str) -> String {
    safe_folder(&folder_name(&os_stem(file_name)))
}

/// `scene` when the name has no stem.
pub fn os_stem(file_name: &str) -> String {
    let name = match file_name.rfind('.') {
        Some(dot) if dot > 0 => &file_name[..dot],
        _ => file_name,
    };
    if name.is_empty() {
        "scene".to_string()
    } else {
        name.to_string()
    }
}

fn base_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A folder that belongs to this drawing and no other path:
/// `<folder of the drawing>/<scene>-vmi`. A bare name is resolved against
/// `cwd`, the way Python's `abspath` does; an unsaved drawing passes its home.
pub fn default_export_parent(path: &Path, cwd: &Path) -> PathBuf {
    let path = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    let name = base_name(&path);
    let folder = path.parent().map(Path::to_path_buf).unwrap_or_else(|| cwd.to_path_buf());
    normalize(&folder.join(format!("{}-vmi", scene_dirname(&name))))
}

/// The scene folder itself: `<parent>/<scene>`.
pub fn export_root(parent: &Path, file_name: &str) -> PathBuf {
    normalize(&parent.join(scene_dirname(file_name)))
}

/// True when two tabs would write the same scene folder.
pub fn same_export(parent_a: &Path, name_a: &str, parent_b: &Path, name_b: &str) -> bool {
    if parent_a.as_os_str().is_empty() || parent_b.as_os_str().is_empty() {
        return false;
    }
    export_root(parent_a, name_a) == export_root(parent_b, name_b)
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_folders_sit_beside_the_drawing() {
        let home = Path::new("/home/me");
        let parent = default_export_parent(Path::new("/work/shop/front.clip"), home);
        assert_eq!(parent, PathBuf::from("/work/shop/front-vmi"));
        assert_eq!(export_root(&parent, "front.clip"), PathBuf::from("/work/shop/front-vmi/front"));
        assert_eq!(default_export_parent(Path::new("Untitled 1920×1080"), home), PathBuf::from("/home/me/Untitled 1920×1080-vmi"));
        assert_eq!(scene_dirname(".vmib"), ".vmib");
        assert_eq!(scene_dirname(""), "scene");
        assert_eq!(scene_dirname("a/b:c.vmib"), "a b c");
        assert!(same_export(Path::new("/out"), "door.vmib", Path::new("/out/./"), "door.clip"));
        assert!(!same_export(Path::new("/out"), "door.vmib", Path::new("/out"), "shop.vmib"));
    }
}
