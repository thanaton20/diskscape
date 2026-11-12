//! Commands that act on files: delete, reveal, and asking for more access.

use crate::dto::display_path;
use crate::state::{AppState, NO_TREE};
use diskscape_core::delete::{
    delete, effective_selection, is_system_path, DeleteError, DeleteMode, DeleteOutcome,
};
use diskscape_core::tree::{NodeId, Tree};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SelectionInfo {
    /// Objects that would be deleted, after dropping duplicates and anything
    /// nested inside another selected directory.
    pub count: usize,
    pub dsize: u64,
    pub has_system_path: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeleteErrorDto {
    /// `readOnly`, `notFound`, `refused`, `trashUnavailable`, `partial` or `io`.
    /// Only `partial` means that something may have been deleted.
    pub kind: &'static str,
    pub message: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub id: NodeId,
    pub ok: bool,
    /// Disk space the tree's total shrank by.
    pub freed: u64,
    pub error: Option<DeleteErrorDto>,
}

// ------------------------------------------------------------ pure logic

pub fn selection_info(tree: &Tree, ids: &[NodeId]) -> SelectionInfo {
    let selection = effective_selection(tree, ids);
    SelectionInfo {
        count: selection.len(),
        dsize: selection.iter().filter_map(|id| tree.node(*id)).map(|n| n.dsize).sum(),
        has_system_path: selection.iter().any(|id| is_system_path(&tree.path(*id))),
    }
}

pub fn delete_result(outcome: &DeleteOutcome) -> DeleteResult {
    let error = outcome.result.as_ref().err().map(|e| match e {
        DeleteError::ReadOnlyTree => DeleteErrorDto {
            kind: "readOnly",
            message: "this view was opened from a file and cannot be changed".into(),
        },
        DeleteError::NotFound => DeleteErrorDto {
            kind: "notFound",
            message: "the object is no longer on disk".into(),
        },
        DeleteError::Refused(why) => DeleteErrorDto { kind: "refused", message: why.clone() },
        DeleteError::TrashUnavailable(why) => DeleteErrorDto { kind: "trashUnavailable", message: why.clone() },
        DeleteError::Partial(why) => DeleteErrorDto { kind: "partial", message: why.clone() },
        DeleteError::Io(why) => DeleteErrorDto { kind: "io", message: why.clone() },
    });
    DeleteResult {
        id: outcome.id,
        ok: error.is_none(),
        freed: outcome.result.as_ref().map_or(0, |freed| *freed),
        error,
    }
}

/// Exactly `"trash"` or `"permanent"`. Anything else is an error: an
/// unrecognised value must never fall through to a delete.
pub fn parse_mode(mode: &str) -> Result<DeleteMode, String> {
    match mode {
        "trash" => Ok(DeleteMode::Trash),
        "permanent" => Ok(DeleteMode::Permanent),
        other => Err(format!("unknown delete mode {other:?}")),
    }
}

/// Node ids are only meaningful inside the tree they came from. If the tree
/// was replaced (a new scan, an import) after the selection was made, the
/// same numbers would point at unrelated files.
pub fn check_tree(tree: &Tree, tree_id: u64) -> Result<(), String> {
    if tree.token() == tree_id {
        Ok(())
    } else {
        Err("the list has changed since this selection was made; nothing was deleted".into())
    }
}

/// Path to show in the file manager; refused for imported trees, whose paths
/// may describe another machine.
pub fn reveal_path(tree: &Tree, id: NodeId) -> Result<PathBuf, String> {
    if tree.read_only() {
        return Err("this view was opened from a file; its paths may not exist here".into());
    }
    tree.node(id)
        .map(|_| PathBuf::from(display_path(&tree.path(id))))
        .ok_or_else(|| "the object is no longer in the list".into())
}

/// `pkexec env VAR=… <exe>`: a root process started by pkexec gets a clean
/// environment, so the variables a GUI needs to reach the display are passed
/// through explicitly.
pub fn pkexec_command(exe: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut cmd = vec!["pkexec".to_string(), "env".to_string()];
    for var in ["DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "XDG_RUNTIME_DIR"] {
        if let Some(value) = env(var) {
            cmd.push(format!("{var}={value}"));
        }
    }
    cmd.push(exe.to_string_lossy().into_owned());
    cmd
}

// ------------------------------------------------------------ commands

#[tauri::command(async)]
pub fn get_selection_info(ids: Vec<NodeId>, state: State<'_, AppState>) -> Result<SelectionInfo, String> {
    let guard = state.tree.read().map_err(|_| "internal state is poisoned".to_string())?;
    let tree = guard.as_ref().ok_or(NO_TREE)?;
    Ok(selection_info(tree, &ids))
}

/// Deletes exactly the given nodes. Only ever called from an explicit user action.
#[tauri::command(async)]
pub fn delete_nodes(
    ids: Vec<NodeId>,
    mode: String,
    tree_id: u64,
    state: State<'_, AppState>,
) -> Result<Vec<DeleteResult>, String> {
    let mode = parse_mode(&mode)?;
    let mut guard = state.tree.write().map_err(|_| "internal state is poisoned".to_string())?;
    let tree = guard.as_mut().ok_or(NO_TREE)?;
    check_tree(tree, tree_id)?;
    Ok(delete(tree, &ids, mode).iter().map(delete_result).collect())
}

#[tauri::command(async)]
pub fn reveal(id: NodeId, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let path = {
        let guard = state.tree.read().map_err(|_| "internal state is poisoned".to_string())?;
        reveal_path(guard.as_ref().ok_or(NO_TREE)?, id)?
    };
    app.opener().reveal_item_in_dir(&path).map_err(|e| e.to_string())
}

/// Helps the user give the app access to what it could not read. The app
/// never asks for a password itself; the operating system does.
///
/// - macOS: opens the Full Disk Access pane of System Settings.
/// - Windows: relaunches elevated through the UAC prompt.
/// - Linux: relaunches through `pkexec`.
#[tauri::command]
pub fn request_elevation(app: AppHandle) -> Result<(), String> {
    elevate(&app)
}

#[cfg(target_os = "macos")]
fn elevate(app: &AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
            None::<&str>,
        )
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
fn elevate(app: &AppHandle) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(std::iter::once(0)).collect() };
    let verb = wide(std::ffi::OsStr::new("runas"));
    let file = wide(exe.as_os_str());
    // SAFETY: both strings are valid NUL-terminated wide strings that outlive the call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW reports success with a value greater than 32.
    if result as usize > 32 {
        app.exit(0);
        Ok(())
    } else {
        Err("the request for administrator rights was declined".into())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn elevate(app: &AppHandle) -> Result<(), String> {
    use tauri::Manager;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let cmd = pkexec_command(&exe, &|name| std::env::var(name).ok());
    let mut child = std::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .spawn()
        .map_err(|e| format!("cannot start pkexec: {e}"))?;

    // Step aside while the elevated copy runs. If authorisation is dismissed
    // (pkexec exits with 126 or 127) come back instead of leaving the user with nothing.
    let windows: Vec<_> = app.webview_windows().into_values().collect();
    for window in &windows {
        let _ = window.hide();
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let declined = matches!(child.wait().ok().and_then(|s| s.code()), Some(126 | 127) | None);
        if declined {
            for window in &windows {
                let _ = window.show();
            }
        } else {
            app.exit(0);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskscape_core::delete::{DeleteError, DeleteOutcome};
    use diskscape_core::tree::{Entry, NodeKind, Tree};
    use std::path::{Path, PathBuf};

    /// `<root>/{a 4096, sub/{b 8192}}` — ids: root 0, a 1, sub 2, b 3
    fn tree(root: &str, read_only: bool) -> Tree {
        let mut t = Tree::new(PathBuf::from(root), Entry::new(root, NodeKind::Dir, 0, 0), read_only);
        let r = t.root();
        t.add_child(r, Entry::new("a", NodeKind::File, 1, 4096));
        let sub = t.add_child(r, Entry::new("sub", NodeKind::Dir, 0, 0));
        t.add_child(sub, Entry::new("b", NodeKind::File, 1, 8192));
        t.finalize();
        t
    }

    #[test]
    fn selection_info_drops_nested_and_sums() {
        let t = tree("/Users/me/data", false);
        let info = selection_info(&t, &[2, 3, 2, 999]);
        assert_eq!((info.count, info.dsize, info.has_system_path), (1, 8192, false));
        let info = selection_info(&t, &[1, 3]);
        assert_eq!((info.count, info.dsize), (2, 4096 + 8192));
        assert_eq!(selection_info(&t, &[]).count, 0);
        assert!(serde_json::to_string(&info).unwrap().contains("\"hasSystemPath\""));
    }

    #[test]
    fn selection_info_flags_system_path() {
        assert!(selection_info(&tree("/usr/local", false), &[1]).has_system_path);
        assert!(selection_info(&tree("/", false), &[2]).has_system_path == false);
        assert!(selection_info(&tree("/", false), &[0]).has_system_path);
        assert!(!selection_info(&tree("/Users/me", false), &[1, 2]).has_system_path);
    }

    #[test]
    fn delete_outcome_maps_error_kinds() {
        let kind = |e: DeleteError| {
            let r = delete_result(&DeleteOutcome { id: 7, result: Err(e) });
            assert!(!r.ok);
            assert_eq!((r.id, r.freed), (7, 0));
            let err = r.error.unwrap();
            assert!(!err.message.is_empty());
            err.kind
        };
        assert_eq!(kind(DeleteError::ReadOnlyTree), "readOnly");
        assert_eq!(kind(DeleteError::NotFound), "notFound");
        assert_eq!(kind(DeleteError::Refused("mount".into())), "refused");
        assert_eq!(kind(DeleteError::TrashUnavailable("no trash".into())), "trashUnavailable");
        assert_eq!(kind(DeleteError::Partial("stopped".into())), "partial");
        assert_eq!(kind(DeleteError::Io("denied".into())), "io");

        let ok = delete_result(&DeleteOutcome { id: 3, result: Ok(8192) });
        assert!(ok.ok && ok.error.is_none());
        assert_eq!(ok.freed, 8192);
        assert!(serde_json::to_string(&ok).unwrap().contains("\"error\":null"));
    }

    #[test]
    fn a_selection_from_another_tree_is_rejected() {
        let (first, second) = (tree("/r", false), tree("/r", false));
        assert!(check_tree(&first, first.token()).is_ok());
        assert!(check_tree(&second, first.token()).is_err());
        assert!(check_tree(&first, 0).is_err());
    }

    #[test]
    fn only_the_two_known_modes_are_accepted() {
        use diskscape_core::delete::DeleteMode;
        assert_eq!(parse_mode("trash"), Ok(DeleteMode::Trash));
        assert_eq!(parse_mode("permanent"), Ok(DeleteMode::Permanent));
        for bad in ["", "Permanent", "delete", "TRASH", "trash "] {
            assert!(parse_mode(bad).is_err(), "{bad:?} must not select a delete mode");
        }
    }

    #[test]
    fn reveal_refused_for_read_only_tree() {
        assert!(reveal_path(&tree("/r", true), 1).is_err());
        assert_eq!(reveal_path(&tree("/r", false), 3).unwrap(), PathBuf::from("/r/sub/b"));
        assert!(reveal_path(&tree("/r", false), 999).is_err());
    }

    #[test]
    fn pkexec_command_passes_display_env() {
        let env = |name: &str| match name {
            "DISPLAY" => Some(":0".to_string()),
            "WAYLAND_DISPLAY" => Some("wayland-0".to_string()),
            "XAUTHORITY" => Some("/home/me/.Xauthority".to_string()),
            "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
            _ => None,
        };
        let cmd = pkexec_command(Path::new("/opt/diskscape/diskscape"), &env);
        assert_eq!(&cmd[..2], ["pkexec", "env"]);
        for var in ["DISPLAY=:0", "WAYLAND_DISPLAY=wayland-0", "XAUTHORITY=/home/me/.Xauthority", "XDG_RUNTIME_DIR=/run/user/1000"] {
            assert!(cmd.iter().any(|a| a == var), "{var} missing in {cmd:?}");
        }
        assert_eq!(cmd.last().unwrap(), "/opt/diskscape/diskscape");

        // variables that are not set are not passed as empty values
        let cmd = pkexec_command(Path::new("/x"), &|_| None);
        assert_eq!(cmd, ["pkexec", "env", "/x"]);
    }
}
