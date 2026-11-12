//! Parallel single-filesystem directory scanner.
//!
//! Worker threads read directories and send each directory's entries to the
//! calling thread, which is the only one that touches the [`Tree`]. Hidden and
//! system files are always included; symbolic links are recorded but never
//! followed; directories on other filesystems are recorded but not entered.

mod meta;

use crate::tree::{Entry, NodeId, NodeKind, Tree};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the progress callback may fire.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

pub struct ScanOptions {
    pub root: PathBuf,
    /// Worker threads; 0 means one per CPU core.
    pub threads: usize,
}

#[derive(Clone, Debug)]
pub struct Progress {
    /// Objects found so far.
    pub items: u64,
    /// Disk usage found so far. Hard links are not de-duplicated here.
    pub bytes: u64,
    /// Directory most recently processed.
    pub current: PathBuf,
}

/// Shared flag that stops a running scan.
#[derive(Clone, Default, Debug)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
pub enum ScanError {
    Cancelled,
    RootUnreadable(io::Error),
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::Cancelled => write!(f, "scan cancelled"),
            ScanError::RootUnreadable(e) => write!(f, "cannot read the scan root: {e}"),
        }
    }
}

impl std::error::Error for ScanError {}

/// Entries of one directory, sent from a worker to the assembling thread.
struct Batch {
    /// Key of the directory these entries belong to.
    dir: u64,
    /// Each entry with the key its own batch will arrive under, if it is a
    /// directory that is being descended into.
    entries: Vec<(Entry, Option<u64>)>,
    read_error: bool,
}

struct Walker {
    root: PathBuf,
    /// Directories to record but not enter.
    skips: Vec<PathBuf>,
    allowed_devs: Vec<u64>,
    cancel: CancelToken,
    next_key: AtomicU64,
    tx: Sender<Batch>,
}

impl Walker {
    /// Reads one directory, reports it, then queues its subdirectories. The
    /// batch is always sent before the subdirectories are queued, so the
    /// assembler has created a node for a directory before its batch arrives.
    fn walk<'s>(&'s self, scope: &rayon::Scope<'s>, path: PathBuf, key: u64) {
        if self.cancel.is_cancelled() {
            return;
        }
        let mut entries = Vec::new();
        let mut subdirs = Vec::new();
        let mut read_error = false;

        match std::fs::read_dir(&path) {
            Err(_) => read_error = true,
            Ok(iter) => {
                for item in iter {
                    let Ok(item) = item else {
                        read_error = true;
                        break;
                    };
                    let name = item.file_name();
                    let child = path.join(&name);
                    if meta::skip_path(&self.root, &child) || self.skips.contains(&child) {
                        // Reached another way, or deliberately left out: keep a
                        // marker so the folder does not look like an ordinary empty one.
                        let mut marker = Entry::new(name, NodeKind::Dir, 0, 0);
                        marker.other_fs = true;
                        entries.push((marker, None));
                        continue;
                    }
                    let mut entry = match meta::read_entry(&child, &name) {
                        Ok(entry) => entry,
                        // Deleted between listing the directory and reading it.
                        Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                        Err(_) => {
                            let mut unknown = Entry::new(name, NodeKind::Other, 0, 0);
                            unknown.read_error = true;
                            entries.push((unknown, None));
                            continue;
                        }
                    };
                    let mut child_key = None;
                    if entry.kind == NodeKind::Dir {
                        if self.allowed_devs.contains(&entry.dev) {
                            let k = self.next_key.fetch_add(1, Ordering::Relaxed);
                            child_key = Some(k);
                            subdirs.push((child, k));
                        } else {
                            entry.other_fs = true;
                        }
                    }
                    entries.push((entry, child_key));
                }
            }
        }

        let batch = Batch { dir: key, entries, read_error };
        if self.tx.send(batch).is_err() {
            return;
        }
        for (child, k) in subdirs {
            scope.spawn(move |s| self.walk(s, child, k));
        }
    }
}

/// Scans `opts.root` and returns the finalized tree.
///
/// `on_progress` is called on the calling thread, at most every 100 ms and
/// once more when the scan completes.
pub fn scan(
    opts: &ScanOptions,
    cancel: &CancelToken,
    on_progress: &(dyn Fn(&Progress) + Sync),
) -> Result<Tree, ScanError> {
    scan_skipping(opts, cancel, on_progress, Vec::new())
}

/// [`scan`], with extra directories that are recorded but not entered.
pub(crate) fn scan_skipping(
    opts: &ScanOptions,
    cancel: &CancelToken,
    on_progress: &(dyn Fn(&Progress) + Sync),
    skips: Vec<PathBuf>,
) -> Result<Tree, ScanError> {
    if cancel.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    // Resolve every symbolic link in the root once, now. All paths in the tree
    // are then real paths, which is what lets a later delete notice that a
    // directory was swapped for a link. (On Windows this also yields the
    // `\\?\` form, which the Win32 layer does not rewrite.)
    let root_path = std::fs::canonicalize(&opts.root).map_err(ScanError::RootUnreadable)?;
    let root_entry = meta::read_root(&root_path, root_path.as_os_str())
        .map_err(ScanError::RootUnreadable)?;
    let root_is_dir = root_entry.kind == NodeKind::Dir;
    let root_dev = root_entry.dev;
    let mut tree = Tree::new(root_path.clone(), root_entry, false);

    let mut items = 0u64;
    let mut bytes = 0u64;

    if root_is_dir {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(opts.threads)
            .build()
            .map_err(|e| ScanError::RootUnreadable(io::Error::other(e)))?;
        let (tx, rx) = mpsc::channel::<Batch>();
        let walker = Walker {
            allowed_devs: meta::allowed_devs(&root_path, root_dev),
            root: root_path.clone(),
            skips,
            cancel: cancel.clone(),
            next_key: AtomicU64::new(1),
            tx,
        };
        let walk_root = root_path.clone();

        std::thread::scope(|threads| {
            threads.spawn(move || {
                pool.scope(|s| walker.walk(s, walk_root, 0));
                // `walker` drops here, closing the channel and ending the loop below.
            });

            let mut nodes: HashMap<u64, NodeId> = HashMap::from([(0, tree.root())]);
            let mut last_report = Instant::now();
            for batch in rx {
                let Some(parent) = nodes.remove(&batch.dir) else {
                    continue;
                };
                if batch.read_error {
                    tree.set_read_error(parent);
                }
                for (entry, key) in batch.entries {
                    items += 1;
                    bytes = bytes.saturating_add(entry.dsize);
                    let id = tree.add_child(parent, entry);
                    if let Some(key) = key {
                        nodes.insert(key, id);
                    }
                }
                if last_report.elapsed() >= PROGRESS_INTERVAL {
                    last_report = Instant::now();
                    on_progress(&Progress { items, bytes, current: tree.path(parent) });
                }
            }
        });
    }

    if cancel.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    tree.finalize();
    on_progress(&Progress { items, bytes, current: root_path });
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{NodeId, NodeKind, Tree};
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tempfile::TempDir;

    /// `a.txt` (5 bytes), `sub/.dotfile` (1), `sub/.hidden/b.bin` (2)
    fn sample() -> TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), "hello").unwrap();
        fs::create_dir_all(tmp.path().join("sub/.hidden")).unwrap();
        fs::write(tmp.path().join("sub/.dotfile"), "x").unwrap();
        fs::write(tmp.path().join("sub/.hidden/b.bin"), "yy").unwrap();
        tmp
    }

    fn run(root: &Path) -> Tree {
        let opts = ScanOptions { root: root.to_path_buf(), threads: 2 };
        scan(&opts, &CancelToken::default(), &|_| {}).unwrap()
    }

    fn find(t: &Tree, rel: &str) -> Option<NodeId> {
        let mut cur = t.root();
        for part in Path::new(rel).components() {
            let name = part.as_os_str();
            cur = *t
                .node(cur)?
                .children
                .iter()
                .find(|c| t.node(**c).is_some_and(|n| n.name == name))?;
        }
        Some(cur)
    }

    fn all_nodes(t: &Tree) -> Vec<NodeId> {
        let mut out = vec![];
        let mut stack = vec![t.root()];
        while let Some(id) = stack.pop() {
            out.push(id);
            stack.extend(t.node(id).unwrap().children.iter().copied());
        }
        out
    }

    #[test]
    fn finds_hidden_files_and_dirs() {
        let tmp = sample();
        let t = run(tmp.path());
        assert!(find(&t, "sub/.dotfile").is_some());
        assert!(find(&t, "sub/.hidden").is_some());
        assert!(find(&t, "sub/.hidden/b.bin").is_some());
        assert_eq!(t.node(t.root()).unwrap().items, 5);
    }

    #[test]
    fn file_sizes_are_read() {
        let tmp = sample();
        let t = run(tmp.path());
        let sum: u64 = all_nodes(&t)
            .into_iter()
            .map(|id| t.node(id).unwrap())
            .filter(|n| n.kind == NodeKind::File)
            .map(|n| n.own_asize)
            .sum();
        assert_eq!(sum, 8);
        assert_eq!(t.node(find(&t, "a.txt").unwrap()).unwrap().own_asize, 5);
        assert!(t.node(t.root()).unwrap().asize >= 8);
    }

    #[test]
    fn paths_point_at_real_files() {
        let tmp = sample();
        let t = run(tmp.path());
        let b = find(&t, "sub/.hidden/b.bin").unwrap();
        // tree paths are fully resolved, so they can be compared with the disk later
        assert_eq!(t.path(b), tmp.path().canonicalize().unwrap().join("sub/.hidden/b.bin"));
        assert!(t.path(b).exists());
        assert!(!t.read_only());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_not_followed() {
        let tmp = sample();
        std::os::unix::fs::symlink(tmp.path().join("sub"), tmp.path().join("loop")).unwrap();
        let t = run(tmp.path());
        let link = t.node(find(&t, "loop").unwrap()).unwrap();
        assert_eq!(link.kind, NodeKind::Symlink);
        assert!(link.children.is_empty());
        assert_eq!(t.node(t.root()).unwrap().items, 6);
    }

    #[cfg(unix)]
    #[test]
    fn hardlink_counted_once() {
        let tmp = sample();
        fs::hard_link(tmp.path().join("a.txt"), tmp.path().join("sub/hard.txt")).unwrap();
        let t = run(tmp.path());
        let a = t.node(find(&t, "a.txt").unwrap()).unwrap();
        let hard = t.node(find(&t, "sub/hard.txt").unwrap()).unwrap();
        assert_eq!(a.nlink, 2);
        assert_eq!((a.dev, a.ino), (hard.dev, hard.ino));
        assert!(!a.flags.hardlink_dup);
        assert!(hard.flags.hardlink_dup);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_dir_is_flagged_not_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = sample();
        let locked = tmp.path().join("sub/.hidden");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let readable_anyway = fs::read_dir(&locked).is_ok(); // running as root
        let t = run(tmp.path());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if readable_anyway {
            return;
        }
        let hidden = t.node(find(&t, "sub/.hidden").unwrap()).unwrap();
        assert!(hidden.flags.read_error);
        assert!(hidden.children.is_empty());
        assert!(t.node(find(&t, "sub").unwrap()).unwrap().flags.child_error);
        assert!(t.node(t.root()).unwrap().flags.child_error);
        assert_eq!(t.unreadable_dirs(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn odd_names_survive() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let tmp = sample();
        let newline = OsStr::from_bytes(b"new\nline \"quoted\"");
        fs::write(tmp.path().join(newline), "n").unwrap();
        // Some filesystems (APFS) refuse names that are not valid UTF-8.
        let invalid = OsStr::from_bytes(b"bad\xffname");
        let invalid_created = fs::write(tmp.path().join(invalid), "i").is_ok();

        let t = run(tmp.path());
        let names: Vec<_> = t
            .node(t.root())
            .unwrap()
            .children
            .iter()
            .map(|c| t.node(*c).unwrap().name.clone())
            .collect();
        assert!(names.iter().any(|n| n == newline));
        if invalid_created {
            assert!(names.iter().any(|n| n == invalid));
        }
    }

    #[test]
    fn deep_tree_is_scanned() {
        let tmp = tempfile::tempdir().unwrap();
        let mut p = tmp.path().to_path_buf();
        for _ in 0..150 {
            p.push("d");
        }
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("leaf"), "z").unwrap();
        let t = run(tmp.path());
        assert_eq!(t.node(t.root()).unwrap().items, 151);
    }

    #[test]
    fn single_file_root_gives_one_node() {
        let tmp = sample();
        let t = run(&tmp.path().join("a.txt"));
        let root = t.node(t.root()).unwrap();
        assert_eq!(root.kind, NodeKind::File);
        assert_eq!(root.own_asize, 5);
        assert!(root.children.is_empty());
    }

    #[test]
    fn skipped_paths_are_recorded_as_another_filesystem() {
        let tmp = sample();
        let root = tmp.path().canonicalize().unwrap();
        let opts = ScanOptions { root: root.clone(), threads: 2 };
        let t = scan_skipping(&opts, &CancelToken::default(), &|_| {}, vec![root.join("sub")]).unwrap();
        let sub = t.node(find(&t, "sub").unwrap()).unwrap();
        assert_eq!(sub.kind, NodeKind::Dir);
        assert!(sub.flags.other_fs);
        assert!(sub.children.is_empty());
        assert_eq!(t.node(t.root()).unwrap().items, 2);
    }

    #[test]
    fn every_tree_has_its_own_token() {
        let tmp = sample();
        assert_ne!(run(tmp.path()).token(), run(tmp.path()).token());
    }

    #[test]
    fn cancel_returns_cancelled() {
        let tmp = sample();
        let cancel = CancelToken::default();
        cancel.cancel();
        assert!(cancel.is_cancelled());
        let opts = ScanOptions { root: tmp.path().to_path_buf(), threads: 2 };
        assert!(matches!(scan(&opts, &cancel, &|_| {}), Err(ScanError::Cancelled)));
    }

    #[test]
    fn missing_root_is_root_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = ScanOptions { root: tmp.path().join("nope"), threads: 1 };
        assert!(matches!(
            scan(&opts, &CancelToken::default(), &|_| {}),
            Err(ScanError::RootUnreadable(_))
        ));
    }

    #[test]
    fn vanished_entry_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let gone = tmp.path().join("gone");
        let err = meta::read_entry(&gone, gone.file_name().unwrap()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn progress_is_reported_at_least_once() {
        let tmp = sample();
        let calls = AtomicU64::new(0);
        let last_items = AtomicU64::new(0);
        let opts = ScanOptions { root: tmp.path().to_path_buf(), threads: 2 };
        scan(&opts, &CancelToken::default(), &|p: &Progress| {
            calls.fetch_add(1, Ordering::SeqCst);
            last_items.store(p.items, Ordering::SeqCst);
        })
        .unwrap();
        assert!(calls.load(Ordering::SeqCst) >= 1);
        assert_eq!(last_items.load(Ordering::SeqCst), 5);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn root_scan_allows_data_volume_and_skips_its_mount_path() {
        use std::os::unix::fs::MetadataExt;
        let root = Path::new("/");
        let root_dev = fs::metadata(root).unwrap().dev();
        assert!(meta::skip_path(root, Path::new("/System/Volumes/Data")));
        assert!(!meta::skip_path(root, Path::new("/System/Volumes")));
        assert!(!meta::skip_path(Path::new("/Users"), Path::new("/System/Volumes/Data")));
        // Depending on the macOS version the two volumes report the same device
        // id or different ones; either way both must be allowed.
        let devs = meta::allowed_devs(root, root_dev);
        assert!(devs.contains(&root_dev));
        assert!(devs.contains(&fs::metadata("/Users").unwrap().dev()));
        assert!(devs.contains(&fs::metadata("/System/Volumes/Data").unwrap().dev()));
        assert_eq!(meta::allowed_devs(Path::new("/tmp"), 42), vec![42]);
    }
}
