//! The notch's file tray: files and folders dropped on the notch wait there
//! until they are opened, shown in their folder or taken out. It keeps
//! their paths only, in memory, and never touches the files.
use std::path::{Path, PathBuf};

/// The most the tray holds; the oldest leaves when one more comes.
pub const CAPACITY: usize = 24;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tray {
    /// Newest first.
    items: Vec<PathBuf>,
}

impl Tray {
    pub fn items(&self) -> &[PathBuf] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Puts what was dropped at the front, in the order it came; one already
    /// there moves to the front instead of showing twice. True if anything
    /// changed.
    pub fn add(&mut self, dropped: impl IntoIterator<Item = PathBuf>) -> bool {
        let before = self.items.clone();
        let mut new: Vec<PathBuf> = Vec::new();
        for path in dropped {
            if path.as_os_str().is_empty() || new.contains(&path) {
                continue;
            }
            new.push(path);
        }
        self.items.retain(|kept| !new.contains(kept));
        new.append(&mut self.items);
        new.truncate(CAPACITY);
        self.items = new;
        self.items != before
    }

    pub fn remove(&mut self, path: &Path) -> bool {
        let before = self.items.len();
        self.items.retain(|kept| kept != path);
        self.items.len() != before
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

/// The name a file shows in the tray: its last part, or the whole path for
/// a drive or a root.
pub fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn dropped_files_wait_newest_first_and_once_each() {
        let mut tray = Tray::default();
        assert!(tray.add(paths(&["a.txt", "b.png"])));
        assert!(tray.add(paths(&["c.pdf", "a.txt", "c.pdf", ""])));
        assert_eq!(tray.items(), paths(&["c.pdf", "a.txt", "b.png"]));
        // The same again changes nothing.
        assert!(!tray.add(paths(&["c.pdf"])));
        assert!(tray.remove(Path::new("a.txt")));
        assert!(!tray.remove(Path::new("a.txt")));
        assert_eq!(tray.items(), paths(&["c.pdf", "b.png"]));
        tray.clear();
        assert!(tray.is_empty());
    }

    #[test]
    fn the_oldest_leave_when_the_tray_is_full() {
        let mut tray = Tray::default();
        for index in 0..CAPACITY + 3 {
            tray.add([PathBuf::from(format!("file-{index}"))]);
        }
        assert_eq!(tray.items().len(), CAPACITY);
        assert_eq!(tray.items()[0], PathBuf::from(format!("file-{}", CAPACITY + 2)));
        assert!(!tray.items().contains(&PathBuf::from("file-0")));
    }

    #[test]
    fn a_file_shows_by_its_name() {
        assert_eq!(name(Path::new("docs/report final.pdf")), "report final.pdf");
        assert_eq!(name(Path::new("/")), "/");
    }
}
