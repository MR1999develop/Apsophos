//! Finding the images among dropped or chosen files and folders.

use std::path::{Path, PathBuf};

/// The images among `paths`, in order: files as they are, folders through
/// the files directly inside them (sorted by name, not recursively). Also
/// returns how many files were skipped because they are not images.
///
/// Reads the first bytes of every file, so it can take a while on a network
/// drive: call it away from the interface thread.
pub fn scan(paths: &[PathBuf], is_image: impl Fn(&Path) -> bool) -> (Vec<PathBuf>, usize) {
    let mut images = Vec::new();
    let mut skipped = 0;
    for p in paths {
        let candidates: Vec<PathBuf> = if p.is_dir() {
            let mut files: Vec<PathBuf> = std::fs::read_dir(p)
                .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).filter(|f| f.is_file()).collect())
                .unwrap_or_default();
            files.sort();
            files
        } else {
            vec![p.clone()]
        };
        for f in candidates {
            if is_image(&f) {
                images.push(f);
            } else {
                skipped += 1;
            }
        }
    }
    (images, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_listed_one_level_sorted() {
        let dir = std::env::temp_dir().join(format!("rd-scan-{}", std::process::id()));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        for name in ["b.tif", "a.jpg", "notes.txt"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        std::fs::write(sub.join("deep.tif"), b"x").unwrap();
        let loose = dir.join("sub").join("deep.tif");
        let is_image = |p: &Path| p.extension().is_some_and(|e| e != "txt");
        let (images, skipped) = scan(&[dir.clone(), loose.clone()], is_image);
        assert_eq!(images, vec![dir.join("a.jpg"), dir.join("b.tif"), loose]);
        assert_eq!(skipped, 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
