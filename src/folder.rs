//! Browsing the images that sit in the same folder as the current one.

use std::path::{Path, PathBuf};

use image::ImageFormat;

/// True if the file extension is a format the `image` crate can decode.
pub fn is_supported(path: &Path) -> bool {
    ImageFormat::from_path(path).is_ok_and(|f| f.reading_enabled())
}

/// All supported images in the folder containing `path`, sorted by name
/// (case-insensitive, like Explorer).
pub fn images_beside(path: &Path) -> Vec<PathBuf> {
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && is_supported(p))
        .collect();
    files.sort_by_cached_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    files
}

/// The entry `step` places away from `current` in `list`, wrapping around.
pub fn neighbor<'a>(list: &'a [PathBuf], current: &Path, step: isize) -> Option<&'a Path> {
    let len = isize::try_from(list.len()).ok().filter(|&n| n > 0)?;
    let idx = list.iter().position(|p| p == current)?;
    let idx = isize::try_from(idx).ok()?;
    let next = usize::try_from((idx + step).rem_euclid(len)).ok()?;
    list.get(next).map(PathBuf::as_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Vec<PathBuf> {
        ["a.png", "b.jpg", "c.gif"].iter().map(PathBuf::from).collect()
    }

    #[test]
    fn is_supported_should_accept_jpeg() {
        assert!(is_supported(Path::new("photo.JPG")));
    }

    #[test]
    fn is_supported_should_reject_text_files() {
        assert!(!is_supported(Path::new("notes.txt")));
    }

    #[test]
    fn neighbor_should_return_next_file() {
        assert_eq!(neighbor(&list(), Path::new("a.png"), 1), Some(Path::new("b.jpg")));
    }

    #[test]
    fn neighbor_should_wrap_to_last_when_going_back_from_first() {
        assert_eq!(neighbor(&list(), Path::new("a.png"), -1), Some(Path::new("c.gif")));
    }

    #[test]
    fn neighbor_should_return_none_when_current_not_in_list() {
        assert_eq!(neighbor(&list(), Path::new("x.png"), 1), None);
    }
}
