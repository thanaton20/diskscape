//! Per-platform file metadata and the rules for staying on one filesystem.

use crate::tree::{Entry, NodeKind};
use std::ffi::OsStr;
use std::io;
use std::path::Path;

/// Reads one filesystem object without following symbolic links.
pub(crate) fn read_entry(path: &Path, name: &OsStr) -> io::Result<Entry> {
    let md = std::fs::symlink_metadata(path)?;
    Ok(entry_from(path, name, &md))
}

/// Reads the scan root, following a symbolic link if the root is one.
pub(crate) fn read_root(path: &Path, name: &OsStr) -> io::Result<Entry> {
    let md = std::fs::metadata(path)?;
    Ok(entry_from(path, name, &md))
}

#[cfg(unix)]
fn entry_from(_path: &Path, name: &OsStr, md: &std::fs::Metadata) -> Entry {
    use std::os::unix::fs::MetadataExt;
    let ft = md.file_type();
    let kind = if ft.is_dir() {
        NodeKind::Dir
    } else if ft.is_file() {
        NodeKind::File
    } else if ft.is_symlink() {
        NodeKind::Symlink
    } else {
        NodeKind::Other
    };
    let mut e = Entry::new(name, kind, md.size(), md.blocks().saturating_mul(512));
    e.dev = md.dev();
    e.ino = md.ino();
    e.nlink = md.nlink();
    e
}

#[cfg(windows)]
fn entry_from(path: &Path, name: &OsStr, md: &std::fs::Metadata) -> Entry {
    let ft = md.file_type();
    // Junctions and symbolic links both report as symlinks and are never followed,
    // which is also what keeps the scan on one volume.
    let kind = if ft.is_symlink() {
        NodeKind::Symlink
    } else if ft.is_dir() {
        NodeKind::Dir
    } else if ft.is_file() {
        NodeKind::File
    } else {
        NodeKind::Other
    };
    if kind != NodeKind::File {
        return Entry::new(name, kind, 0, 0);
    }
    let asize = md.len();
    let dsize = windows::size_on_disk(path).unwrap_or(asize);
    let mut e = Entry::new(name, kind, asize, dsize);
    if let Some(id) = windows::file_id(path) {
        e.dev = id.volume;
        e.ino = id.index;
        e.nlink = id.links;
    }
    e
}

#[cfg(not(any(unix, windows)))]
fn entry_from(_path: &Path, name: &OsStr, md: &std::fs::Metadata) -> Entry {
    let ft = md.file_type();
    let kind = if ft.is_dir() {
        NodeKind::Dir
    } else if ft.is_file() {
        NodeKind::File
    } else if ft.is_symlink() {
        NodeKind::Symlink
    } else {
        NodeKind::Other
    };
    Entry::new(name, kind, md.len(), md.len())
}

/// Device ids a scan started at `root` may descend into.
///
/// On macOS the startup disk is two volumes glued together with firmlinks —
/// the sealed system volume at `/` and the data volume — so a scan of `/`
/// covers both.
pub(crate) fn allowed_devs(root: &Path, root_dev: u64) -> Vec<u64> {
    #[cfg(target_os = "macos")]
    if root == Path::new("/") {
        use std::os::unix::fs::MetadataExt;
        if let Ok(md) = std::fs::metadata(MACOS_DATA_VOLUME) {
            if md.dev() != root_dev {
                return vec![root_dev, md.dev()];
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = root;
    vec![root_dev]
}

/// Paths that must not be visited because their contents are already reached
/// another way: the macOS data volume's own mount point during a scan of `/`.
pub(crate) fn skip_path(root: &Path, path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        root == Path::new("/") && path == Path::new(MACOS_DATA_VOLUME)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, path);
        false
    }
}

#[cfg(target_os = "macos")]
const MACOS_DATA_VOLUME: &str = "/System/Volumes/Data";

#[cfg(windows)]
mod windows {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, INVALID_HANDLE_VALUE, NO_ERROR,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetCompressedFileSizeW, GetFileInformationByHandle,
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, INVALID_FILE_SIZE, OPEN_EXISTING,
    };

    pub(super) struct FileId {
        pub volume: u64,
        pub index: u64,
        pub links: u64,
    }

    /// NUL-terminated wide path in the `\\?\` form, so paths longer than
    /// MAX_PATH work.
    fn wide(path: &Path) -> Vec<u16> {
        let raw: Vec<u16> = path.as_os_str().encode_wide().collect();
        let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
        let unc: Vec<u16> = r"\\".encode_utf16().collect();
        let mut out = if raw.starts_with(&verbatim) || !path.is_absolute() {
            raw
        } else if raw.starts_with(&unc) {
            let mut v: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
            v.extend_from_slice(&raw[2..]);
            v
        } else {
            let mut v = verbatim;
            v.extend_from_slice(&raw);
            v
        };
        out.push(0);
        out
    }

    /// Bytes the file really occupies (accounts for compression and sparseness).
    pub(super) fn size_on_disk(path: &Path) -> Option<u64> {
        let w = wide(path);
        let mut high: u32 = 0;
        // SAFETY: `w` is a valid NUL-terminated wide string and `high` outlives the call.
        let low = unsafe { GetCompressedFileSizeW(w.as_ptr(), &mut high) };
        // SAFETY: plain thread-local error query.
        if low == INVALID_FILE_SIZE && unsafe { GetLastError() } != NO_ERROR {
            return None;
        }
        Some((u64::from(high) << 32) | u64::from(low))
    }

    /// Volume serial, file index and link count, used to recognise hard links.
    pub(super) fn file_id(path: &Path) -> Option<FileId> {
        let w = wide(path);
        // SAFETY: `w` is a valid NUL-terminated wide string; no access rights are
        // requested, so this opens metadata only.
        let handle = unsafe {
            CreateFileW(
                w.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        // SAFETY: zero is a valid bit pattern for this plain-data struct.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: `handle` is open and `info` is a valid out-pointer.
        let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
        // SAFETY: `handle` was returned by CreateFileW and is closed exactly once.
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return None;
        }
        Some(FileId {
            volume: u64::from(info.dwVolumeSerialNumber),
            index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            links: u64::from(info.nNumberOfLinks),
        })
    }
}
