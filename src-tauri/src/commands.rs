//! Commands for scanning and browsing. The logic lives in plain functions
//! over a `Tree` so it can be tested without a window.

use crate::dto::{display_path, node_dto, rect_dto, NodeDto, ProgressDto, RectDto, ScanFailed, ScanFinished, Summary, VolumeDto};
use crate::state::{AppState, NO_TREE};
use diskscape_core::scan::{scan, CancelToken, ScanError, ScanOptions};
use diskscape_core::tree::{NodeId, SortKey, Tree};
use diskscape_core::{ncdu, treemap as core_treemap};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};

// ------------------------------------------------------------ pure logic

pub fn children(tree: &Tree, id: NodeId, sort: &str, desc: bool) -> Vec<NodeDto> {
    let key = match sort {
        "name" => SortKey::Name,
        "items" => SortKey::Items,
        _ => SortKey::Size,
    };
    tree.children_sorted(id, key, desc)
        .into_iter()
        .filter_map(|child| node_dto(tree, child))
        .collect()
}

pub fn nodes(tree: &Tree, ids: &[NodeId]) -> Vec<Option<NodeDto>> {
    ids.iter().map(|id| node_dto(tree, *id)).collect()
}

pub fn ancestors(tree: &Tree, id: NodeId) -> Vec<NodeDto> {
    tree.ancestors(id).into_iter().filter_map(|a| node_dto(tree, a)).collect()
}

pub fn treemap(tree: &Tree, id: NodeId, width: f32, height: f32) -> Vec<RectDto> {
    core_treemap::layout(tree, id, width, height).iter().map(|r| rect_dto(tree, r)).collect()
}

pub fn summary(tree: &Tree, elapsed_ms: u64) -> Summary {
    Summary {
        unreadable_dirs: tree.unreadable_dirs(),
        total_dsize: tree.node(tree.root()).map_or(0, |n| n.dsize),
        elapsed_ms,
        read_only: tree.read_only(),
        root_path: display_path(&tree.path(tree.root())),
        tree_id: tree.token(),
    }
}

pub fn import_file(path: &Path) -> Result<Tree, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    ncdu::import(file).map_err(|e| e.to_string())
}

pub fn export_file(tree: &Tree, path: &Path) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    ncdu::export(tree, file).map_err(|e| format!("{}: {e}", path.display()))
}

/// Mounted disks worth offering as scan targets.
pub fn volumes() -> Vec<VolumeDto> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<VolumeDto> = disks
        .list()
        .iter()
        .filter(|d| d.total_space() > 0)
        // macOS lists the data volume of the startup disk separately; a scan of "/" covers it.
        .filter(|d| !d.mount_point().starts_with("/System/Volumes"))
        .filter(|d| seen.insert(d.mount_point().to_path_buf()))
        .map(|d| VolumeDto {
            name: d.name().to_string_lossy().into_owned(),
            mount_point: d.mount_point().to_string_lossy().into_owned(),
            total: d.total_space(),
            free: d.available_space(),
        })
        .collect();
    out.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    out
}

/// The folder to scan at launch, from `diskscape <path>`: the first argument
/// after the program name that is an existing path. Options are passed over.
pub fn pick_initial_path(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter()
        .skip(1)
        .find(|arg| !arg.starts_with('-') && Path::new(arg).exists())
}

fn with_tree<T>(state: &AppState, f: impl FnOnce(&Tree) -> T) -> Result<T, String> {
    let guard = state.tree.read().map_err(|_| "internal state is poisoned".to_string())?;
    guard.as_ref().map(f).ok_or_else(|| NO_TREE.to_string())
}

// ------------------------------------------------------------ commands

#[tauri::command]
pub fn initial_path() -> Option<String> {
    pick_initial_path(std::env::args())
}

#[tauri::command(async)]
pub fn list_volumes() -> Vec<VolumeDto> {
    volumes()
}

/// Starts a scan in the background and returns at once. Results arrive as
/// events: `scan-progress`, then one of `scan-finished`, `scan-failed`,
/// `scan-cancelled`. Starting a new scan cancels the previous one silently.
#[tauri::command]
pub fn start_scan(path: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let token = CancelToken::default();
    {
        let mut current = state.cancel.lock().map_err(|_| "internal state is poisoned".to_string())?;
        current.cancel();
        *current = token.clone();
    }
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    *state.tree.write().map_err(|_| "internal state is poisoned".to_string())? = None;

    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let is_current = || state.generation.load(Ordering::SeqCst) == generation;
        let started = Instant::now();
        let opts = ScanOptions { root: PathBuf::from(path), threads: 0 };
        let result = scan(&opts, &token, &|progress| {
            if is_current() {
                let _ = app.emit("scan-progress", ProgressDto::from(progress));
            }
        });
        if !is_current() {
            return;
        }
        match result {
            Ok(tree) => {
                let root = node_dto(&tree, tree.root());
                state.elapsed_ms.store(started.elapsed().as_millis() as u64, Ordering::SeqCst);
                if let Ok(mut slot) = state.tree.write() {
                    *slot = Some(tree);
                }
                match root {
                    Some(root) => {
                        let _ = app.emit("scan-finished", ScanFinished { root });
                    }
                    None => {
                        let _ = app.emit("scan-failed", ScanFailed { message: "empty result".into() });
                    }
                }
            }
            Err(ScanError::Cancelled) => {
                let _ = app.emit("scan-cancelled", ());
            }
            Err(e) => {
                let _ = app.emit("scan-failed", ScanFailed { message: e.to_string() });
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) {
    if let Ok(token) = state.cancel.lock() {
        token.cancel();
    }
}

#[tauri::command(async)]
pub fn get_children(id: NodeId, sort: String, desc: bool, state: State<'_, AppState>) -> Result<Vec<NodeDto>, String> {
    with_tree(&state, |t| children(t, id, &sort, desc))
}

#[tauri::command(async)]
pub fn get_nodes(ids: Vec<NodeId>, state: State<'_, AppState>) -> Result<Vec<Option<NodeDto>>, String> {
    with_tree(&state, |t| nodes(t, &ids))
}

#[tauri::command(async)]
pub fn get_ancestors(id: NodeId, state: State<'_, AppState>) -> Result<Vec<NodeDto>, String> {
    with_tree(&state, |t| ancestors(t, id))
}

#[tauri::command(async)]
pub fn get_path(id: NodeId, state: State<'_, AppState>) -> Result<String, String> {
    with_tree(&state, |t| display_path(&t.path(id)))
}

#[tauri::command(async)]
pub fn get_treemap(id: NodeId, width: f32, height: f32, state: State<'_, AppState>) -> Result<Vec<RectDto>, String> {
    with_tree(&state, |t| treemap(t, id, width, height))
}

#[tauri::command(async)]
pub fn get_summary(state: State<'_, AppState>) -> Result<Summary, String> {
    let elapsed = state.elapsed_ms.load(Ordering::SeqCst);
    with_tree(&state, |t| summary(t, elapsed))
}

#[tauri::command(async)]
pub fn import_ncdu(path: String, state: State<'_, AppState>) -> Result<NodeDto, String> {
    let tree = import_file(Path::new(&path))?;
    let root = node_dto(&tree, tree.root()).ok_or("empty export")?;
    // An import replaces whatever scan is running or shown.
    if let Ok(token) = state.cancel.lock() {
        token.cancel();
    }
    state.generation.fetch_add(1, Ordering::SeqCst);
    state.elapsed_ms.store(0, Ordering::SeqCst);
    *state.tree.write().map_err(|_| "internal state is poisoned".to_string())? = Some(tree);
    Ok(root)
}

#[tauri::command(async)]
pub fn export_ncdu(path: String, state: State<'_, AppState>) -> Result<(), String> {
    with_tree(&state, |t| export_file(t, Path::new(&path)))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskscape_core::tree::{Entry, NodeKind, Tree};
    use std::path::PathBuf;

    /// `/r/{big 9000, small 512, sub/{x 100, y 200}}`
    fn tree() -> Tree {
        let mut t = Tree::new(PathBuf::from("/r"), Entry::new("/r", NodeKind::Dir, 0, 0), false);
        let root = t.root();
        t.add_child(root, Entry::new("small", NodeKind::File, 1, 512));
        t.add_child(root, Entry::new("big", NodeKind::File, 1, 9000));
        let sub = t.add_child(root, Entry::new("sub", NodeKind::Dir, 0, 0));
        t.add_child(sub, Entry::new("x", NodeKind::File, 1, 100));
        t.add_child(sub, Entry::new("y", NodeKind::File, 1, 200));
        t.finalize();
        t
    }

    fn names(nodes: &[crate::dto::NodeDto]) -> Vec<&str> {
        nodes.iter().map(|n| n.name.as_str()).collect()
    }

    #[test]
    fn children_of_fixture_sorted_by_size() {
        let t = tree();
        assert_eq!(names(&children(&t, t.root(), "size", true)), ["big", "small", "sub"]);
        assert_eq!(names(&children(&t, t.root(), "name", false)), ["big", "small", "sub"]);
        assert_eq!(names(&children(&t, t.root(), "items", true)), ["sub", "big", "small"]);
        // an unknown sort key falls back to size
        assert_eq!(names(&children(&t, t.root(), "bogus", true)), ["big", "small", "sub"]);
        assert!(children(&t, 999, "size", true).is_empty());
    }

    #[test]
    fn get_nodes_returns_null_for_removed() {
        let mut t = tree();
        t.remove(1);
        let got = nodes(&t, &[0, 1, 2, 999]);
        assert!(got[0].is_some() && got[2].is_some());
        assert!(got[1].is_none() && got[3].is_none());
        assert_eq!(serde_json::to_string(&got[1]).unwrap(), "null");
    }

    #[test]
    fn ancestors_run_from_root_to_parent() {
        let t = tree();
        assert_eq!(names(&ancestors(&t, 4)), ["/r", "sub"]);
        assert!(ancestors(&t, t.root()).is_empty());
    }

    #[test]
    fn treemap_returns_rect_dtos() {
        let t = tree();
        let rects = treemap(&t, t.root(), 800.0, 600.0);
        assert!(rects.iter().any(|r| r.id == Some(2)));
        assert!(rects.iter().all(|r| r.w >= 0.0 && r.h >= 0.0));
        assert!(treemap(&t, t.root(), 0.0, 0.0).is_empty());
    }

    #[test]
    fn summary_reports_totals_and_root() {
        let t = tree();
        let s = summary(&t, 1234);
        assert_eq!((s.unreadable_dirs, s.total_dsize, s.elapsed_ms, s.read_only), (0, 9812, 1234, false));
        assert_eq!(s.root_path, "/r");
        assert_eq!(s.tree_id, t.token());
        assert_ne!(summary(&tree(), 0).tree_id, s.tree_id);
        assert!(serde_json::to_string(&s).unwrap().contains("\"unreadableDirs\""));
    }

    #[test]
    fn import_then_summary_is_read_only() {
        let mut buf = Vec::new();
        diskscape_core::ncdu::export(&tree(), &mut buf).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), &buf).unwrap();
        let imported = import_file(file.path()).unwrap();
        assert!(summary(&imported, 0).read_only);
        assert_eq!(summary(&imported, 0).total_dsize, 9812);
    }

    #[test]
    fn import_of_a_bad_file_is_an_error_message() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"not an export").unwrap();
        assert!(import_file(file.path()).is_err());
        assert!(import_file(std::path::Path::new("/definitely/not/here.json")).is_err());
    }

    #[test]
    fn initial_path_is_the_first_existing_argument() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().to_string_lossy().into_owned();
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_initial_path(args(&["diskscape", &existing])), Some(existing.clone()));
        // options and things that are not there are passed over
        assert_eq!(pick_initial_path(args(&["diskscape", "--flag", "/no/such/dir", &existing])), Some(existing));
        assert_eq!(pick_initial_path(args(&["diskscape"])), None);
        assert_eq!(pick_initial_path(args(&["diskscape", "/no/such/dir"])), None);
        assert_eq!(pick_initial_path(args(&[])), None);
    }

    #[test]
    fn export_writes_a_file_ncdu_format() {
        let out = tempfile::NamedTempFile::new().unwrap();
        export_file(&tree(), out.path()).unwrap();
        assert!(std::fs::read_to_string(out.path()).unwrap().starts_with("[1,2,{"));
    }
}
