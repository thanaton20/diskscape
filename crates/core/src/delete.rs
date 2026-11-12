//! Deleting scanned objects: to the system Trash, or permanently.
//!
//! Rules this module enforces no matter what the caller asks for:
//!
//! - nothing in a read-only (imported) tree is touched;
//! - a failed move to the Trash is reported and never turns into a permanent delete;
//! - the scan root is never deleted;
//! - a directory that is, or contains, a mount point of another filesystem is
//!   never deleted — it looks empty in the tree but holds a whole volume;
//! - symbolic links are removed themselves, never followed.

use crate::tree::{NodeId, Tree};
use std::collections::HashSet;
use std::io;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeleteMode {
    /// Move to the system Trash; reversible.
    Trash,
    /// Remove from disk; irreversible.
    Permanent,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DeleteError {
    /// The tree was imported from a file and is view-only.
    ReadOnlyTree,
    /// The object is no longer on disk (or no longer in the tree).
    NotFound,
    /// Deleting this object is never allowed; the message says why.
    Refused(String),
    /// The Trash could not take the object. Nothing was deleted.
    TrashUnavailable(String),
    Io(String),
}

#[derive(Debug)]
pub struct DeleteOutcome {
    pub id: NodeId,
    /// On success, how much the tree's total disk usage shrank — zero when
    /// another hard link to the same file is still in the tree.
    pub result: Result<u64, DeleteError>,
}

/// Deletes the selected nodes and updates the tree's totals.
///
/// Duplicate ids and ids nested inside another selected directory are dropped,
/// so each remaining id gets exactly one outcome, in selection order. One
/// failure does not stop the rest.
pub fn delete(tree: &mut Tree, ids: &[NodeId], mode: DeleteMode) -> Vec<DeleteOutcome> {
    delete_with(tree, ids, mode, &trash_item)
}

/// The selection with duplicates, ids no longer in the tree, and ids that sit
/// inside another selected directory removed. Order is preserved.
pub fn effective_selection(tree: &Tree, ids: &[NodeId]) -> Vec<NodeId> {
    let selected: HashSet<NodeId> = ids.iter().copied().collect();
    let mut seen = HashSet::new();
    ids.iter()
        .copied()
        .filter(|id| seen.insert(*id))
        .filter(|id| tree.node(*id).is_some())
        .filter(|id| !tree.ancestors(*id).iter().any(|a| selected.contains(a)))
        .collect()
}

pub(crate) fn delete_with(
    tree: &mut Tree,
    ids: &[NodeId],
    mode: DeleteMode,
    trash: &dyn Fn(&Path) -> Result<(), String>,
) -> Vec<DeleteOutcome> {
    // Classify the whole selection before touching anything: deleting a
    // directory removes its selected children from the tree as a side effect.
    let keep: HashSet<NodeId> = effective_selection(tree, ids).into_iter().collect();
    let gone: HashSet<NodeId> = ids.iter().copied().filter(|id| tree.node(*id).is_none()).collect();
    let mut seen = HashSet::new();
    let mut outcomes = Vec::new();
    for &id in ids {
        if !seen.insert(id) {
            continue;
        }
        let result = if tree.read_only() {
            Err(DeleteError::ReadOnlyTree)
        } else if gone.contains(&id) {
            Err(DeleteError::NotFound)
        } else if !keep.contains(&id) {
            continue; // covered by a selected ancestor
        } else {
            delete_one(tree, id, mode, trash)
        };
        outcomes.push(DeleteOutcome { id, result });
    }
    outcomes
}

fn delete_one(
    tree: &mut Tree,
    id: NodeId,
    mode: DeleteMode,
    trash: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<u64, DeleteError> {
    if id == tree.root() {
        return Err(DeleteError::Refused(
            "the scanned folder itself cannot be deleted".into(),
        ));
    }
    if contains_mount_point(tree, id) {
        return Err(DeleteError::Refused(
            "this is, or contains, a mount point of another filesystem".into(),
        ));
    }

    let path = tree.path(id);
    // What is on disk now decides how to remove it; the scan may be stale.
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(md) => md,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            tree.remove(id);
            return Err(DeleteError::NotFound);
        }
        Err(e) => return Err(DeleteError::Io(e.to_string())),
    };

    match mode {
        DeleteMode::Trash => trash(&path).map_err(DeleteError::TrashUnavailable)?,
        DeleteMode::Permanent => {
            remove_from_disk(&path, &metadata).map_err(|e| DeleteError::Io(e.to_string()))?
        }
    }
    Ok(tree.remove(id))
}

fn contains_mount_point(tree: &Tree, id: NodeId) -> bool {
    let mut stack = vec![id];
    while let Some(cur) = stack.pop() {
        let Some(node) = tree.node(cur) else { continue };
        if node.flags.other_fs {
            return true;
        }
        stack.extend(node.children.iter().copied());
    }
    false
}

/// Removes a real directory recursively and anything else — including a
/// symbolic link to a directory — as a single entry. `remove_dir_all` itself
/// does not follow symbolic links either.
fn remove_from_disk(path: &Path, metadata: &std::fs::Metadata) -> io::Result<()> {
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        return std::fs::remove_dir_all(path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        if file_type.is_symlink_dir() {
            return std::fs::remove_dir(path);
        }
    }
    std::fs::remove_file(path)
}

fn trash_item(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // NSFileManager rather than the default Finder scripting: it needs no
        // Automation permission prompt and works in headless sessions.
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        let mut ctx = trash::TrashContext::default();
        ctx.set_delete_method(DeleteMethod::NsFileManager);
        ctx.delete(path).map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map_err(|e| e.to_string())
    }
}

const UNIX_SYSTEM_DIRS: [&str; 12] = [
    "System", "Library", "usr", "bin", "sbin", "etc", "var", "private", "opt", "boot", "lib",
    "lib64",
];

const WINDOWS_SYSTEM_DIRS: [&str; 4] =
    ["Windows", "Program Files", "Program Files (x86)", "ProgramData"];

/// True for a filesystem root, an operating-system directory, or anything
/// inside one. Used to add a warning before deleting, never to block.
/// Both Unix and Windows path styles are recognised on every platform.
pub fn is_system_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    if let Some(rest) = windows_drive_rest(&text) {
        return match rest.split(['\\', '/']).find(|part| !part.is_empty()) {
            None => true,
            Some(first) => WINDOWS_SYSTEM_DIRS.iter().any(|dir| dir.eq_ignore_ascii_case(first)),
        };
    }
    if !text.starts_with('/') {
        return false;
    }
    match text.split('/').find(|part| !part.is_empty()) {
        None => true,
        Some(first) => UNIX_SYSTEM_DIRS.contains(&first),
    }
}

/// For `C:\x` or `\\?\C:\x`, the part after the drive letter and colon.
fn windows_drive_rest(text: &str) -> Option<&str> {
    let text = text.strip_prefix(r"\\?\").unwrap_or(text);
    let bytes = text.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':').then(|| &text[2..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{scan, CancelToken, ScanOptions};
    use crate::tree::{Entry, NodeId, NodeKind, Tree};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    /// `a.txt`, `sub/b.bin`, `sub/deep/c.txt`
    fn sample() -> TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("sub/deep")).unwrap();
        fs::write(tmp.path().join("a.txt"), "hello").unwrap();
        fs::write(tmp.path().join("sub/b.bin"), vec![1u8; 9000]).unwrap();
        fs::write(tmp.path().join("sub/deep/c.txt"), "c").unwrap();
        tmp
    }

    fn scanned(root: &Path) -> Tree {
        let opts = ScanOptions { root: root.to_path_buf(), threads: 2 };
        scan(&opts, &CancelToken::default(), &|_| {}).unwrap()
    }

    fn find(t: &Tree, rel: &str) -> NodeId {
        let mut cur = t.root();
        for part in Path::new(rel).components() {
            let name = part.as_os_str();
            cur = *t
                .node(cur)
                .unwrap()
                .children
                .iter()
                .find(|c| t.node(**c).is_some_and(|n| n.name == name))
                .unwrap_or_else(|| panic!("{rel} not in tree"));
        }
        cur
    }

    fn root_dsize(t: &Tree) -> u64 {
        t.node(t.root()).unwrap().dsize
    }

    #[test]
    fn permanent_removes_file_from_disk_and_tree() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let a = find(&t, "a.txt");
        let out = delete(&mut t, &[a], DeleteMode::Permanent);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, a);
        assert!(out[0].result.is_ok());
        assert!(!tmp.path().join("a.txt").exists());
        assert!(t.node(a).is_none());
        assert!(tmp.path().join("sub/b.bin").exists());
    }

    #[test]
    fn permanent_removes_directory_recursively() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let sub = find(&t, "sub");
        let out = delete(&mut t, &[sub], DeleteMode::Permanent);
        assert!(out[0].result.is_ok());
        assert!(!tmp.path().join("sub").exists());
        assert!(tmp.path().join("a.txt").exists());
        assert_eq!(t.node(t.root()).unwrap().items, 1);
    }

    #[test]
    fn totals_update_after_delete() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let before = root_dsize(&t);
        let b = find(&t, "sub/b.bin");
        let size = t.node(b).unwrap().dsize;
        assert!(size > 0);
        let out = delete(&mut t, &[b], DeleteMode::Permanent);
        assert_eq!(out[0].result.as_ref().ok(), Some(&size));
        assert_eq!(root_dsize(&t), before - size);
    }

    #[test]
    fn nested_selection_deletes_parent_once() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let sub = find(&t, "sub");
        let b = find(&t, "sub/b.bin");
        let c = find(&t, "sub/deep/c.txt");
        let out = delete(&mut t, &[b, sub, c, sub], DeleteMode::Permanent);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, sub);
        assert!(out[0].result.is_ok());
        assert!(!tmp.path().join("sub").exists());
    }

    #[test]
    fn already_missing_is_not_found_and_removed_from_tree() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let a = find(&t, "a.txt");
        fs::remove_file(tmp.path().join("a.txt")).unwrap();
        let out = delete(&mut t, &[a], DeleteMode::Permanent);
        assert!(matches!(out[0].result, Err(DeleteError::NotFound)));
        assert!(t.node(a).is_none());
    }

    #[test]
    fn id_no_longer_in_tree_is_not_found() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let a = find(&t, "a.txt");
        delete(&mut t, &[a], DeleteMode::Permanent);
        let out = delete(&mut t, &[a, 999_999], DeleteMode::Permanent);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|o| matches!(o.result, Err(DeleteError::NotFound))));
    }

    #[test]
    fn read_only_tree_refuses() {
        let tmp = sample();
        let mut buf = Vec::new();
        crate::ncdu::export(&scanned(tmp.path()), &mut buf).unwrap();
        let mut t = crate::ncdu::import(&buf[..]).unwrap();
        let a = find(&t, "a.txt");
        for mode in [DeleteMode::Trash, DeleteMode::Permanent] {
            let out = delete(&mut t, &[a], mode);
            assert!(matches!(out[0].result, Err(DeleteError::ReadOnlyTree)));
        }
        assert!(tmp.path().join("a.txt").exists());
        assert!(t.node(a).is_some());
    }

    #[test]
    fn failure_on_one_item_does_not_stop_others() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let a = find(&t, "a.txt");
        let b = find(&t, "sub/b.bin");
        fs::remove_file(tmp.path().join("a.txt")).unwrap();
        let out = delete(&mut t, &[a, b], DeleteMode::Permanent);
        assert!(out[0].result.is_err());
        assert!(out[1].result.is_ok());
        assert!(!tmp.path().join("sub/b.bin").exists());
    }

    #[cfg(unix)]
    #[test]
    fn permission_failure_is_io_and_keeps_the_node() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let b = find(&t, "sub/b.bin");
        let sub = tmp.path().join("sub");
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o555)).unwrap();
        let writable_anyway = fs::write(sub.join("probe"), "").is_ok(); // running as root
        let _ = fs::remove_file(sub.join("probe"));
        let out = delete(&mut t, &[b], DeleteMode::Permanent);
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
        if writable_anyway {
            return;
        }
        assert!(matches!(out[0].result, Err(DeleteError::Io(_))));
        assert!(sub.join("b.bin").exists());
        assert!(t.node(b).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn deletes_odd_names() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let tmp = sample();
        let newline = OsStr::from_bytes(b"new\nline \"q\"");
        fs::write(tmp.path().join(newline), "n").unwrap();
        let invalid = OsStr::from_bytes(b"bad\xffname");
        let invalid_created = fs::write(tmp.path().join(invalid), "i").is_ok();
        let mut t = scanned(tmp.path());

        let mut ids = vec![find(&t, newline.to_str().unwrap())];
        if invalid_created {
            let root = t.node(t.root()).unwrap();
            ids.push(*root.children.iter().find(|c| t.node(**c).unwrap().name == invalid).unwrap());
        }
        let out = delete(&mut t, &ids, DeleteMode::Permanent);
        assert!(out.iter().all(|o| o.result.is_ok()));
        assert!(!tmp.path().join(newline).exists());
        assert!(!tmp.path().join(invalid).exists());
    }

    #[cfg(unix)]
    #[test]
    fn deleting_a_symlink_keeps_its_target() {
        let tmp = sample();
        std::os::unix::fs::symlink(tmp.path().join("sub"), tmp.path().join("link")).unwrap();
        let mut t = scanned(tmp.path());
        let link = find(&t, "link");
        let out = delete(&mut t, &[link], DeleteMode::Permanent);
        assert!(out[0].result.is_ok());
        assert!(fs::symlink_metadata(tmp.path().join("link")).is_err());
        assert!(tmp.path().join("sub/b.bin").exists());
        assert!(tmp.path().join("sub/deep/c.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn directory_replaced_by_symlink_since_scan_is_not_followed() {
        let tmp = sample();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("precious"), "p").unwrap();
        let mut t = scanned(tmp.path());
        let deep = find(&t, "sub/deep");
        fs::remove_dir_all(tmp.path().join("sub/deep")).unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("sub/deep")).unwrap();
        let out = delete(&mut t, &[deep], DeleteMode::Permanent);
        assert!(out[0].result.is_ok());
        assert!(outside.path().join("precious").exists());
        assert!(fs::symlink_metadata(tmp.path().join("sub/deep")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn deleting_one_hard_link_frees_nothing_until_the_last() {
        let tmp = sample();
        fs::hard_link(tmp.path().join("sub/b.bin"), tmp.path().join("b2.bin")).unwrap();
        let mut t = scanned(tmp.path());
        let size = t.node(find(&t, "b2.bin")).unwrap().own_dsize;
        let before = root_dsize(&t);
        let (first, second) = (find(&t, "b2.bin"), find(&t, "sub/b.bin"));
        let out = delete(&mut t, &[first], DeleteMode::Permanent);
        assert_eq!(out[0].result.as_ref().ok(), Some(&0));
        assert_eq!(root_dsize(&t), before);
        let out = delete(&mut t, &[second], DeleteMode::Permanent);
        assert_eq!(out[0].result.as_ref().ok(), Some(&size));
    }

    #[test]
    fn scan_root_is_refused() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let root = t.root();
        for mode in [DeleteMode::Trash, DeleteMode::Permanent] {
            let out = delete(&mut t, &[root], mode);
            assert!(matches!(out[0].result, Err(DeleteError::Refused(_))));
        }
        assert!(tmp.path().join("a.txt").exists());
    }

    #[test]
    fn mount_points_and_folders_containing_them_are_refused() {
        // A directory on another filesystem shows up as an empty-looking folder;
        // deleting it (or its parent) would wipe a whole mounted volume.
        let tmp = sample();
        let mut t = Tree::new(
            tmp.path().to_path_buf(),
            Entry::new(tmp.path().as_os_str(), NodeKind::Dir, 0, 0),
            false,
        );
        let sub = t.add_child(t.root(), Entry::new("sub", NodeKind::Dir, 0, 0));
        let mut mount = Entry::new("deep", NodeKind::Dir, 0, 0);
        mount.other_fs = true;
        let mount = t.add_child(sub, mount);
        t.finalize();

        let out = delete(&mut t, &[mount], DeleteMode::Permanent);
        assert!(matches!(out[0].result, Err(DeleteError::Refused(_))));
        let out = delete(&mut t, &[sub], DeleteMode::Permanent);
        assert!(matches!(out[0].result, Err(DeleteError::Refused(_))));
        assert!(tmp.path().join("sub/deep/c.txt").exists());
        assert!(t.node(sub).is_some());
    }

    #[test]
    fn trash_failure_never_deletes_permanently() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let a = find(&t, "a.txt");
        let sub = find(&t, "sub");
        let no_trash = |_: &Path| Err("no trash on this volume".to_string());
        let out = delete_with(&mut t, &[a, sub], DeleteMode::Trash, &no_trash);
        assert_eq!(out.len(), 2);
        for o in &out {
            assert!(matches!(&o.result, Err(DeleteError::TrashUnavailable(m)) if m.contains("no trash")));
        }
        assert!(tmp.path().join("a.txt").exists());
        assert!(tmp.path().join("sub/deep/c.txt").exists());
        assert!(t.node(a).is_some());
        assert!(t.node(sub).is_some());
    }

    #[test]
    fn trash_mode_uses_the_trash_and_never_removes_itself() {
        let tmp = sample();
        let holding = tempfile::tempdir().unwrap();
        let mut t = scanned(tmp.path());
        let a = find(&t, "a.txt");
        let before = root_dsize(&t);
        let fake_trash = |p: &Path| {
            fs::rename(p, holding.path().join(p.file_name().unwrap())).map_err(|e| e.to_string())
        };
        let out = delete_with(&mut t, &[a], DeleteMode::Trash, &fake_trash);
        assert!(out[0].result.is_ok());
        assert!(holding.path().join("a.txt").exists());
        assert!(t.node(a).is_none());
        assert!(root_dsize(&t) < before);
    }

    #[test]
    fn system_paths() {
        for p in ["/", "/usr", "/usr/lib/x", "/System", "/System/Library/a", "/Library/Fonts", "/etc/hosts",
                  "/private/var/db", "/bin/sh", "/sbin", "/opt/homebrew", "/boot/grub", "/lib64/ld.so"] {
            assert!(is_system_path(Path::new(p)), "{p} should be a system path");
        }
        for p in ["/Users/me/usr", "/Users/me/Library/Caches", "/home/me/bin", "/usrlocal", "/Systems", "relative/usr"] {
            assert!(!is_system_path(Path::new(p)), "{p} should not be a system path");
        }
        for p in [r"C:\Windows", r"c:\windows\System32\x.dll", r"D:\Program Files\App", r"C:\Program Files (x86)",
                  r"C:\ProgramData\x", r"C:\", r"\\?\C:\Windows\x"] {
            assert!(is_system_path(Path::new(p)), "{p} should be a system path");
        }
        for p in [r"C:\Users\me\Windows", r"D:\Games", r"C:\WindowsOld"] {
            assert!(!is_system_path(Path::new(p)), "{p} should not be a system path");
        }
    }

    /// Uses the real system Trash, so it leaves one small file there.
    #[test]
    #[ignore]
    fn trash_moves_file_out() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let a = find(&t, "a.txt");
        let out = delete(&mut t, &[a], DeleteMode::Trash);
        assert!(out[0].result.is_ok(), "{:?}", out[0].result);
        assert!(!tmp.path().join("a.txt").exists());
        assert!(t.node(a).is_none());
    }
}
