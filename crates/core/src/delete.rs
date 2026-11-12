//! Deleting scanned objects: to the system Trash, or permanently.
//!
//! The tree is a snapshot; the disk may have changed since. This module
//! deletes only what it can prove is still the object that was scanned, and
//! refuses otherwise. Rules it enforces no matter what the caller asks for:
//!
//! - nothing in a read-only (imported) tree is touched;
//! - a failed move to the Trash is reported and never turns into a permanent delete;
//! - the scan root is never deleted;
//! - a directory that is, or contains, a mount point of another filesystem is
//!   never deleted — checked against the scan and again against the disk;
//! - a directory whose contents could not be read at scan time is never
//!   deleted: it looks empty in the tree but is not;
//! - an object is deleted only if its parent directory is still the real
//!   directory that was scanned and the object has the same kind and identity
//!   — so a folder swapped for a symbolic link, or a file replaced by another,
//!   is left alone;
//! - a permanent delete that stops part-way says so and marks the node stale.

use crate::tree::{NodeId, NodeKind, Tree};
use std::collections::HashSet;
use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};

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
    /// Deleting this object is not allowed; the message says why. Nothing was deleted.
    Refused(String),
    /// The Trash could not take the object. Nothing was deleted.
    TrashUnavailable(String),
    /// A permanent delete of a directory stopped part-way: some of its
    /// contents may already be gone. The node is kept and flagged as stale.
    Partial(String),
    /// The operation failed. Nothing was deleted.
    Io(String),
}

#[derive(Debug)]
pub struct DeleteOutcome {
    pub id: NodeId,
    /// On success, how much the tree's total disk usage shrank — zero when
    /// another hard link to the same file is still in the tree.
    pub result: Result<u64, DeleteError>,
}

const CHANGED: &str = "it has changed on disk since the scan; scan again before deleting it";
const MOUNT: &str = "this is, or contains, a mount point of another filesystem";
const UNREAD: &str =
    "part of it could not be read when it was scanned, so its real contents are unknown; scan again";

/// The outside world, replaceable in tests.
pub(crate) struct Hooks<'a> {
    pub trash: &'a dyn Fn(&Path) -> Result<(), String>,
    /// Device id of an object, used to notice filesystem boundaries.
    pub device_of: &'a dyn Fn(&Path, &Metadata) -> u64,
    /// Every mount point the operating system currently reports.
    pub mount_points: &'a dyn Fn() -> Vec<PathBuf>,
}

/// Deletes the selected nodes and updates the tree's totals.
///
/// Duplicate ids and ids nested inside another selected directory are dropped,
/// so each remaining id gets exactly one outcome, in selection order. One
/// failure does not stop the rest.
pub fn delete(tree: &mut Tree, ids: &[NodeId], mode: DeleteMode) -> Vec<DeleteOutcome> {
    let hooks = Hooks {
        trash: &trash_item,
        device_of: &real_device,
        mount_points: &system_mount_points,
    };
    delete_with_hooks(tree, ids, mode, &hooks)
}

#[cfg(test)]
pub(crate) fn delete_with(
    tree: &mut Tree,
    ids: &[NodeId],
    mode: DeleteMode,
    trash: &dyn Fn(&Path) -> Result<(), String>,
) -> Vec<DeleteOutcome> {
    let hooks = Hooks { trash, device_of: &real_device, mount_points: &system_mount_points };
    delete_with_hooks(tree, ids, mode, &hooks)
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

pub(crate) fn delete_with_hooks(
    tree: &mut Tree,
    ids: &[NodeId],
    mode: DeleteMode,
    hooks: &Hooks,
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
            delete_one(tree, id, mode, hooks)
        };
        outcomes.push(DeleteOutcome { id, result });
    }
    outcomes
}

fn delete_one(tree: &mut Tree, id: NodeId, mode: DeleteMode, hooks: &Hooks) -> Result<u64, DeleteError> {
    let refused = |why: &str| Err(DeleteError::Refused(why.to_string()));

    // What the scan already knows.
    if id == tree.root() {
        return refused("the scanned folder itself cannot be deleted");
    }
    let Some(node) = tree.node(id) else {
        return Err(DeleteError::NotFound);
    };
    if contains_mount_point(tree, id) {
        return refused(MOUNT);
    }
    if node.flags.read_error || node.flags.child_error {
        return refused(UNREAD);
    }

    // What is on disk now.
    let path = tree.path(id);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(md) => md,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            tree.remove(id);
            return Err(DeleteError::NotFound);
        }
        Err(e) => return Err(DeleteError::Io(e.to_string())),
    };
    check_unchanged(tree, id, &path, &metadata, hooks)?;
    if metadata.is_dir() {
        check_single_filesystem(&path, &metadata, hooks)?;
    }

    match mode {
        DeleteMode::Trash => {
            check_trashable(&path)?;
            (hooks.trash)(&path).map_err(DeleteError::TrashUnavailable)?;
        }
        DeleteMode::Permanent => {
            if let Err(e) = remove_from_disk(&path, &metadata) {
                if !metadata.is_dir() {
                    return Err(DeleteError::Io(e.to_string()));
                }
                // Some of the directory may already be gone; the totals in the
                // tree can no longer be trusted, so say so instead of pretending
                // nothing happened.
                tree.set_read_error(id);
                tree.finalize();
                return Err(DeleteError::Partial(format!(
                    "{e}; part of it may already be deleted — scan again to see what is left"
                )));
            }
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

/// The object at `path` must still be the one that was scanned: reached
/// through the same real directories, of the same kind, with the same identity.
fn check_unchanged(
    tree: &Tree,
    id: NodeId,
    path: &Path,
    metadata: &Metadata,
    hooks: &Hooks,
) -> Result<(), DeleteError> {
    let changed = || DeleteError::Refused(CHANGED.to_string());
    let node = tree.node(id).ok_or(DeleteError::NotFound)?;

    // Tree paths are fully resolved at scan time. If resolving the parent now
    // gives a different path, some directory above was replaced by a link and
    // `path` leads somewhere that was never scanned.
    let parent = path.parent().ok_or_else(changed)?;
    match std::fs::canonicalize(parent) {
        Ok(real) if real == parent => {}
        Ok(_) => return Err(changed()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(changed()),
        Err(e) => return Err(DeleteError::Io(e.to_string())),
    }

    if metadata.is_dir() != (node.kind == NodeKind::Dir) {
        return Err(changed());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (hooks.device_of)(path, metadata) != node.dev || metadata.ino() != node.ino {
            return Err(changed());
        }
    }
    #[cfg(not(unix))]
    let _ = hooks;
    Ok(())
}

/// Refuses a directory that now holds a mount point: either the system's
/// mount table lists one inside it, or a directory below it lives on another
/// device. Runs before anything is unlinked.
fn check_single_filesystem(path: &Path, metadata: &Metadata, hooks: &Hooks) -> Result<(), DeleteError> {
    if mounts_within(path, &(hooks.mount_points)()) {
        return Err(DeleteError::Refused(MOUNT.to_string()));
    }
    let device = (hooks.device_of)(path, metadata);
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        // A directory that cannot be listed cannot be emptied either; the
        // delete will stop there and report a partial failure.
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let child = entry.path();
            let Ok(md) = std::fs::symlink_metadata(&child) else { continue };
            if md.is_dir() {
                if (hooks.device_of)(&child, &md) != device {
                    return Err(DeleteError::Refused(MOUNT.to_string()));
                }
                stack.push(child);
            }
        }
    }
    Ok(())
}

/// True if any mount point is `target` itself or lies below it.
pub(crate) fn mounts_within(target: &Path, mounts: &[PathBuf]) -> bool {
    mounts.iter().any(|mount| mount.starts_with(target))
}

pub(crate) fn real_device(_path: &Path, metadata: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.dev()
    }
    #[cfg(not(unix))]
    {
        // Not available without opening the object. On Windows a recursive
        // delete does not pass through mount points or junctions anyway.
        let _ = metadata;
        0
    }
}

/// Mount points from the operating system. Needed on Linux, where a bind
/// mount keeps the device id of its source and only the mount table shows it.
fn system_mount_points() -> Vec<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/mountinfo")
            .map(|text| parse_mountinfo(&text))
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// Mount points listed in Linux's `/proc/self/mountinfo`: the fifth field of
/// each line, with `\040`-style octal escapes decoded.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_mountinfo(text: &str) -> Vec<PathBuf> {
    fn unescape(field: &str) -> String {
        let bytes = field.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let octal = (bytes[i] == b'\\' && i + 4 <= bytes.len())
                .then(|| std::str::from_utf8(&bytes[i + 1..i + 4]).ok())
                .flatten()
                .and_then(|digits| u8::from_str_radix(digits, 8).ok());
            match octal {
                Some(byte) => {
                    out.push(byte);
                    i += 4;
                }
                None => {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            // A real line has the mount point in field 5 and a " - " separator later.
            (fields.len() >= 7 && fields.contains(&"-")).then(|| PathBuf::from(unescape(fields[4])))
        })
        .collect()
}

/// Removes a real directory recursively and anything else — including a
/// symbolic link to a directory — as a single entry. `remove_dir_all` itself
/// does not follow symbolic links either.
fn remove_from_disk(path: &Path, metadata: &Metadata) -> io::Result<()> {
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

/// Names the system Trash would mishandle are turned away before it is asked.
fn check_trashable(path: &Path) -> Result<(), DeleteError> {
    #[cfg(windows)]
    if path.file_name().is_some_and(|name| win32_renames(&name.to_string_lossy())) {
        return Err(DeleteError::Refused(
            "Windows cannot move a name like this to the Recycle Bin without changing it".into(),
        ));
    }
    #[cfg(target_os = "macos")]
    if path.to_str().is_none() {
        return Err(DeleteError::TrashUnavailable(
            "the name is not valid UTF-8, which the macOS Trash cannot handle".into(),
        ));
    }
    let _ = path;
    Ok(())
}

/// True for a file name the Win32 layer silently alters: one ending in a dot
/// or a space, or a reserved device name such as `CON` or `aux.txt`. The
/// Recycle Bin goes through that layer, so such a name could reach another file.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn win32_renames(name: &str) -> bool {
    if name.ends_with('.') || name.ends_with(' ') {
        return true;
    }
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix)
            .is_some_and(|rest| rest.len() == 1 && matches!(rest.as_bytes()[0], b'1'..=b'9'))
    };
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered("COM") || numbered("LPT")
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
    pub(super) fn sample() -> TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("sub/deep")).unwrap();
        fs::write(tmp.path().join("a.txt"), "hello").unwrap();
        fs::write(tmp.path().join("sub/b.bin"), vec![1u8; 9000]).unwrap();
        fs::write(tmp.path().join("sub/deep/c.txt"), "c").unwrap();
        tmp
    }

    pub(super) fn scanned(root: &Path) -> Tree {
        let opts = ScanOptions { root: root.to_path_buf(), threads: 2 };
        scan(&opts, &CancelToken::default(), &|_| {}).unwrap()
    }

    pub(super) fn find(t: &Tree, rel: &str) -> NodeId {
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

    pub(super) fn root_dsize(t: &Tree) -> u64 {
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
        assert!(matches!(out[0].result, Err(DeleteError::Refused(_))));
        assert!(outside.path().join("precious").exists());
        assert!(fs::symlink_metadata(tmp.path().join("sub/deep")).is_ok());
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

/// Regression tests for the independent review of this module: every case
/// where the disk no longer matches what was scanned.
#[cfg(test)]
mod stale_tree_tests {
    use super::tests::{find, sample, scanned};
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn real_trash(_: &Path) -> Result<(), String> {
        panic!("the trash must not be reached in this test")
    }

    fn no_mounts() -> Vec<PathBuf> {
        Vec::new()
    }

    #[test]
    fn mount_that_appeared_after_the_scan_is_refused() {
        // `sub/deep` was a plain directory when scanned; now it reports another device.
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let (sub, deep) = (find(&t, "sub"), find(&t, "sub/deep"));
        let fake_device = |path: &Path, md: &fs::Metadata| {
            if path.ends_with("sub/deep") { 999_999 } else { real_device(path, md) }
        };
        let hooks = Hooks { trash: &real_trash, device_of: &fake_device, mount_points: &no_mounts };
        for mode in [DeleteMode::Permanent, DeleteMode::Trash] {
            for id in [sub, deep] {
                let out = delete_with_hooks(&mut t, &[id], mode, &hooks);
                assert!(matches!(out[0].result, Err(DeleteError::Refused(_))), "{:?}", out[0].result);
            }
        }
        assert!(tmp.path().join("sub/deep/c.txt").exists());
        assert!(tmp.path().join("sub/b.bin").exists());
        assert!(t.node(sub).is_some());
    }

    #[test]
    fn mount_point_listed_by_the_system_is_refused_even_on_the_same_device() {
        // Linux bind mounts keep the device id; only the mount table shows them.
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let (sub, deep, a) = (find(&t, "sub"), find(&t, "sub/deep"), find(&t, "a.txt"));
        let mount = t.path(deep);
        let mounts = move || vec![mount.clone(), PathBuf::from("/"), PathBuf::from("/somewhere/else")];
        let hooks = Hooks { trash: &real_trash, device_of: &real_device, mount_points: &mounts };
        for id in [sub, deep] {
            let out = delete_with_hooks(&mut t, &[id], DeleteMode::Permanent, &hooks);
            assert!(matches!(out[0].result, Err(DeleteError::Refused(_))));
        }
        assert!(tmp.path().join("sub/deep/c.txt").exists());
        // unrelated mounts do not get in the way
        let out = delete_with_hooks(&mut t, &[a], DeleteMode::Permanent, &hooks);
        assert!(out[0].result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn ancestor_replaced_by_a_symlink_is_refused() {
        let tmp = sample();
        let outside = tempfile::tempdir().unwrap();
        let mut t = scanned(tmp.path());
        let (deep, b) = (find(&t, "sub/deep"), find(&t, "sub/b.bin"));
        fs::rename(tmp.path().join("sub"), outside.path().join("sub")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("sub"), tmp.path().join("sub")).unwrap();
        for mode in [DeleteMode::Permanent, DeleteMode::Trash] {
            let hooks = Hooks { trash: &real_trash, device_of: &real_device, mount_points: &no_mounts };
            let out = delete_with_hooks(&mut t, &[deep, b], mode, &hooks);
            assert!(out.iter().all(|o| matches!(o.result, Err(DeleteError::Refused(_)))), "{out:?}");
        }
        assert!(outside.path().join("sub/deep/c.txt").exists());
        assert!(outside.path().join("sub/b.bin").exists());
    }

    #[cfg(unix)]
    #[test]
    fn root_reached_through_a_symlink_is_pinned_at_scan_time() {
        let dirs = tempfile::tempdir().unwrap();
        for d in ["a", "b"] {
            fs::create_dir(dirs.path().join(d)).unwrap();
            fs::write(dirs.path().join(d).join("data"), d).unwrap();
        }
        let link = dirs.path().join("link");
        std::os::unix::fs::symlink(dirs.path().join("a"), &link).unwrap();
        let mut t = scanned(&link);
        assert_eq!(t.path(t.root()), dirs.path().join("a").canonicalize().unwrap());

        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(dirs.path().join("b"), &link).unwrap();
        let data = find(&t, "data");
        let out = delete(&mut t, &[data], DeleteMode::Permanent);
        assert!(out[0].result.is_ok());
        assert!(!dirs.path().join("a/data").exists());
        assert!(dirs.path().join("b/data").exists());
    }

    #[test]
    fn file_replaced_by_a_directory_is_refused() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let a = find(&t, "a.txt");
        fs::remove_file(tmp.path().join("a.txt")).unwrap();
        fs::create_dir(tmp.path().join("a.txt")).unwrap();
        fs::write(tmp.path().join("a.txt/important"), "i").unwrap();
        let out = delete(&mut t, &[a], DeleteMode::Permanent);
        assert!(matches!(out[0].result, Err(DeleteError::Refused(_))));
        assert!(tmp.path().join("a.txt/important").exists());
    }

    #[cfg(unix)]
    #[test]
    fn file_replaced_by_another_file_is_refused() {
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let a = find(&t, "a.txt");
        // Both files exist at once, so the replacement cannot reuse the inode.
        fs::write(tmp.path().join("a.new"), "a newer document").unwrap();
        fs::rename(tmp.path().join("a.new"), tmp.path().join("a.txt")).unwrap();
        let out = delete(&mut t, &[a], DeleteMode::Permanent);
        assert!(matches!(out[0].result, Err(DeleteError::Refused(_))));
        assert_eq!(fs::read_to_string(tmp.path().join("a.txt")).unwrap(), "a newer document");
    }

    #[cfg(unix)]
    #[test]
    fn directory_that_could_not_be_read_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = sample();
        let locked = tmp.path().join("sub/deep");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let readable_anyway = fs::read_dir(&locked).is_ok(); // running as root
        let mut t = scanned(tmp.path());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if readable_anyway {
            return;
        }
        // It shows as empty in the tree, but it is not.
        let (deep, sub) = (find(&t, "sub/deep"), find(&t, "sub"));
        assert_eq!(t.node(deep).unwrap().items, 0);
        for mode in [DeleteMode::Permanent, DeleteMode::Trash] {
            let hooks = Hooks { trash: &real_trash, device_of: &real_device, mount_points: &no_mounts };
            for id in [deep, sub] {
                let out = delete_with_hooks(&mut t, &[id], mode, &hooks);
                assert!(matches!(out[0].result, Err(DeleteError::Refused(_))), "{:?}", out[0].result);
            }
        }
        assert!(tmp.path().join("sub/deep/c.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn partial_failure_is_reported_and_marks_the_node_stale() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = sample();
        let mut t = scanned(tmp.path());
        let sub = find(&t, "sub");
        let deep = tmp.path().join("sub/deep");
        fs::set_permissions(&deep, fs::Permissions::from_mode(0o555)).unwrap();
        let writable_anyway = fs::write(deep.join("probe"), "").is_ok(); // running as root
        let _ = fs::remove_file(deep.join("probe"));
        let out = delete(&mut t, &[sub], DeleteMode::Permanent);
        fs::set_permissions(&deep, fs::Permissions::from_mode(0o755)).unwrap();
        if writable_anyway {
            return;
        }
        assert!(matches!(out[0].result, Err(DeleteError::Partial(_))), "{:?}", out[0].result);
        assert!(deep.join("c.txt").exists());
        // The node stays, flagged as no longer trustworthy, and cannot be deleted again blindly.
        assert!(t.node(sub).unwrap().flags.read_error);
        assert!(t.node(t.root()).unwrap().flags.child_error);
        let again = delete(&mut t, &[sub], DeleteMode::Permanent);
        assert!(matches!(again[0].result, Err(DeleteError::Refused(_))));
    }

    #[test]
    fn mountinfo_is_parsed_with_escapes() {
        let text = "\
36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue
40 36 0:35 / /home/me/My\\040Drive rw - fuse.rclone remote rw
41 36 8:1 /data /srv/bind\\134slash rw shared:2 - ext4 /dev/sda1 rw
garbage line
";
        assert_eq!(
            parse_mountinfo(text),
            vec![PathBuf::from("/mnt2"), PathBuf::from("/home/me/My Drive"), PathBuf::from("/srv/bind\\slash")]
        );
        assert!(parse_mountinfo("").is_empty());
    }

    #[test]
    fn mounts_within_matches_the_path_itself_and_anything_below() {
        let mounts = [PathBuf::from("/"), PathBuf::from("/home/me/mnt/disk"), PathBuf::from("/data")];
        assert!(mounts_within(Path::new("/home/me/mnt"), &mounts));
        assert!(mounts_within(Path::new("/home/me/mnt/disk"), &mounts));
        assert!(mounts_within(Path::new("/data"), &mounts));
        assert!(!mounts_within(Path::new("/home/me/mnt/disk/inner"), &mounts));
        assert!(!mounts_within(Path::new("/home/me/mntx"), &mounts));
        assert!(!mounts_within(Path::new("/home/me"), &[PathBuf::from("/")]));
    }

    #[test]
    fn names_windows_would_silently_change_are_recognised() {
        for name in ["report.", "report ", "CON", "nul", "com1", "LPT9", "aux.txt", "con.tar.gz", "..."] {
            assert!(win32_renames(name), "{name:?}");
        }
        for name in ["report", "report.txt", ".hidden", "console", "com10", "a b", "lpt"] {
            assert!(!win32_renames(name), "{name:?}");
        }
    }

    /// Uses the real system Trash, so it leaves a few small items there.
    #[test]
    #[ignore]
    fn trash_takes_a_directory_and_a_symlink_without_touching_the_target() {
        let tmp = sample();
        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.path().join("sub"), tmp.path().join("link")).unwrap();
        let mut t = scanned(tmp.path());
        #[cfg(unix)]
        {
            let link = find(&t, "link");
            let out = delete(&mut t, &[link], DeleteMode::Trash);
            assert!(out[0].result.is_ok(), "{:?}", out[0].result);
            assert!(fs::symlink_metadata(tmp.path().join("link")).is_err());
            assert!(tmp.path().join("sub/deep/c.txt").exists());
        }
        let sub = find(&t, "sub");
        let out = delete(&mut t, &[sub], DeleteMode::Trash);
        assert!(out[0].result.is_ok(), "{:?}", out[0].result);
        assert!(!tmp.path().join("sub").exists());
        assert!(t.node(sub).is_none());
    }
}
