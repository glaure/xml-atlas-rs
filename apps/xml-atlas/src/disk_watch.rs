//! Looks at the files of the open documents on a thread of its own and wakes the window only
//! when one of them changed (`docs/spec/open.md`, OPEN-14). An idle window stays idle.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use xml_atlas_engine::FileStamp;

/// How often the files are looked at.
const INTERVAL: Duration = Duration::from_secs(1);

/// The files to look at, and whether one changed since the window last asked.
pub struct DiskWatch {
    shared: Arc<Shared>,
    /// The paths last handed to the thread.
    paths: Vec<PathBuf>,
}

#[derive(Default)]
struct Shared {
    paths: Mutex<Vec<PathBuf>>,
    changed: AtomicBool,
}

impl DiskWatch {
    /// Starts the thread; it ends when the watch is dropped.
    pub fn start(ctx: egui::Context) -> Self {
        let shared = Arc::new(Shared::default());
        let weak = Arc::downgrade(&shared);
        let spawned = std::thread::Builder::new()
            .name("disk-watch".into())
            .spawn(move || run(&weak, &ctx));
        if let Err(e) = spawned {
            log::warn!("cannot watch files for changes: {e}");
        }
        Self {
            shared,
            paths: Vec::new(),
        }
    }

    /// Looks at these files from now on. True when the list differs from the last one, so
    /// the caller can compare the new files once itself.
    pub fn watch(&mut self, paths: Vec<PathBuf>) -> bool {
        if paths == self.paths {
            return false;
        }
        if let Ok(mut shared) = self.shared.paths.lock() {
            shared.clone_from(&paths);
        }
        self.paths = paths;
        true
    }

    /// Whether a file changed since the last call.
    pub fn take_changed(&self) -> bool {
        self.shared.changed.swap(false, Ordering::Relaxed)
    }
}

/// Every [`INTERVAL`], compares each file with how it looked the time before; on a difference
/// flags it and wakes the window. A file seen for the first time is only noted.
fn run(shared: &Weak<Shared>, ctx: &egui::Context) {
    let mut seen: HashMap<PathBuf, Option<FileStamp>> = HashMap::new();
    loop {
        std::thread::sleep(INTERVAL);
        let Some(shared) = shared.upgrade() else {
            return;
        };
        let paths = match shared.paths.lock() {
            Ok(p) => p.clone(),
            Err(_) => return,
        };
        let mut changed = false;
        let mut now = HashMap::with_capacity(paths.len());
        for path in paths {
            let Ok(stamp) = FileStamp::of(&path) else {
                continue;
            };
            changed |= seen.get(&path).is_some_and(|s| *s != stamp);
            now.insert(path, stamp);
        }
        seen = now;
        if changed {
            shared.changed.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_flagged_and_a_new_file_is_not() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("größe.xml");
        std::fs::write(&path, "<a/>").unwrap();
        let mut watch = DiskWatch::start(egui::Context::default());
        assert!(watch.watch(vec![path.clone()]));
        assert!(!watch.watch(vec![path.clone()]), "the same list");
        std::thread::sleep(INTERVAL * 2 + INTERVAL / 2);
        assert!(!watch.take_changed(), "a file seen for the first time");
        std::fs::write(&path, "<a>\r\n</a>").unwrap();
        std::thread::sleep(INTERVAL + INTERVAL / 2);
        assert!(watch.take_changed());
        assert!(!watch.take_changed(), "taken");
        std::fs::remove_file(&path).unwrap();
        std::thread::sleep(INTERVAL + INTERVAL / 2);
        assert!(watch.take_changed(), "removed");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
