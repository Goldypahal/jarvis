//! File snapshots so every change the agent makes can be undone (SRS FR-62).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug)]
struct Entry {
    path: PathBuf,
    /// Copy of the file before the change; `None` if the file did not exist.
    backup: Option<PathBuf>,
}

/// Ordered log of snapshots taken during a session.
#[derive(Debug)]
pub struct Journal {
    dir: PathBuf,
    entries: Mutex<Vec<Entry>>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct UndoReport {
    /// Files put back to their earlier content.
    pub restored: usize,
    /// Files the agent created, now deleted.
    pub removed: usize,
}

impl Journal {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            entries: Mutex::new(Vec::new()),
        }
    }

    /// A position in the journal; pass it to [`Journal::undo_to`] later.
    pub fn mark(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    pub fn len(&self) -> usize {
        self.mark()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Record the current state of `path` before it is changed.
    pub fn snapshot(&self, path: &Path) -> std::io::Result<()> {
        let mut entries = self.entries.lock().unwrap();
        let backup = if path.is_file() {
            std::fs::create_dir_all(&self.dir)?;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let dest = self.dir.join(format!("{:05}_{name}", entries.len()));
            std::fs::copy(path, &dest)?;
            Some(dest)
        } else {
            None
        };
        entries.push(Entry {
            path: path.to_path_buf(),
            backup,
        });
        Ok(())
    }

    /// Undo every change made after `mark`, newest first.
    pub fn undo_to(&self, mark: usize) -> std::io::Result<UndoReport> {
        let mut entries = self.entries.lock().unwrap();
        let mut report = UndoReport::default();
        while entries.len() > mark {
            let e = entries.pop().expect("len > mark");
            match &e.backup {
                Some(b) => {
                    std::fs::copy(b, &e.path)?;
                    report.restored += 1;
                }
                None => {
                    if e.path.exists() {
                        std::fs::remove_file(&e.path)?;
                    }
                    report.removed += 1;
                }
            }
        }
        Ok(report)
    }
}
