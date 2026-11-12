//! Shapes sent to the interface. Field names are camelCase on the wire.

use diskscape_core::category::categorize;
use diskscape_core::scan::Progress;
use diskscape_core::tree::{NodeId, NodeKind, Tree};
use diskscape_core::treemap::Rect;
use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeDto {
    pub id: NodeId,
    /// Lossy UTF-8; the real name stays in the tree and is used for all file operations.
    pub name: String,
    pub kind: &'static str,
    /// Disk usage including descendants.
    pub dsize: u64,
    /// Apparent size including descendants.
    pub asize: u64,
    /// Number of descendants.
    pub items: u64,
    pub has_children: bool,
    pub category: &'static str,
    pub read_error: bool,
    pub child_error: bool,
    pub other_fs: bool,
    pub hardlink_dup: bool,
}

pub fn node_dto(tree: &Tree, id: NodeId) -> Option<NodeDto> {
    let n = tree.node(id)?;
    Some(NodeDto {
        id,
        name: n.name.to_string_lossy().into_owned(),
        kind: match n.kind {
            NodeKind::Dir => "dir",
            NodeKind::File => "file",
            NodeKind::Symlink => "symlink",
            NodeKind::Other => "other",
        },
        dsize: n.dsize,
        asize: n.asize,
        items: n.items,
        has_children: !n.children.is_empty(),
        category: categorize(&n.name).as_str(),
        read_error: n.flags.read_error,
        child_error: n.flags.child_error,
        other_fs: n.flags.other_fs,
        hardlink_dup: n.flags.hardlink_dup,
    })
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RectDto {
    /// `null` for the block that stands for all items too small to draw.
    pub id: Option<NodeId>,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub depth: u8,
    pub is_dir: bool,
    pub category: &'static str,
}

impl From<&Rect> for RectDto {
    fn from(r: &Rect) -> Self {
        RectDto {
            id: r.id,
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
            depth: r.depth,
            is_dir: r.is_dir,
            category: r.category.as_str(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VolumeDto {
    pub name: String,
    pub mount_point: String,
    pub total: u64,
    pub free: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProgressDto {
    pub items: u64,
    pub bytes: u64,
    pub current: String,
}

impl From<&Progress> for ProgressDto {
    fn from(p: &Progress) -> Self {
        ProgressDto {
            items: p.items,
            bytes: p.bytes,
            current: p.current.to_string_lossy().into_owned(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub unreadable_dirs: u64,
    pub total_dsize: u64,
    pub elapsed_ms: u64,
    /// True for a tree imported from a file: nothing in it may be deleted or revealed.
    pub read_only: bool,
    pub root_path: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanFinished {
    pub root: NodeDto,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanFailed {
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskscape_core::tree::{Entry, NodeKind, Tree};
    use std::path::PathBuf;

    fn tree() -> Tree {
        let mut t = Tree::new(PathBuf::from("/r"), Entry::new("/r", NodeKind::Dir, 0, 0), false);
        let root = t.root();
        t.add_child(root, Entry::new("movie.mkv", NodeKind::File, 5, 4096));
        let sub = t.add_child(root, Entry::new("sub", NodeKind::Dir, 0, 0));
        let mut locked = Entry::new("locked", NodeKind::Dir, 0, 0);
        locked.read_error = true;
        t.add_child(sub, locked);
        t.finalize();
        t
    }

    #[test]
    fn node_dto_serializes_camel_case() {
        let t = tree();
        let json = serde_json::to_string(&node_dto(&t, t.root()).unwrap()).unwrap();
        for key in ["\"hasChildren\"", "\"readError\"", "\"childError\"", "\"otherFs\"", "\"hardlinkDup\"", "\"dsize\"", "\"items\""] {
            assert!(json.contains(key), "{key} missing in {json}");
        }
        assert!(!json.contains("has_children"));
    }

    #[test]
    fn node_dto_carries_kind_category_and_flags() {
        let t = tree();
        let root = node_dto(&t, t.root()).unwrap();
        assert_eq!((root.kind, root.name.as_str(), root.has_children), ("dir", "/r", true));
        assert_eq!((root.dsize, root.items), (4096, 3));
        assert!(root.child_error && !root.read_error);
        let movie = node_dto(&t, 1).unwrap();
        assert_eq!((movie.kind, movie.category, movie.has_children), ("file", "video", false));
        assert!(node_dto(&t, 999).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn name_is_lossy_utf8_but_never_panics() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let mut t = Tree::new(PathBuf::from("/r"), Entry::new("/r", NodeKind::Dir, 0, 0), false);
        let id = t.add_child(t.root(), Entry::new(OsStr::from_bytes(b"bad\xffname"), NodeKind::File, 1, 1));
        t.finalize();
        let dto = node_dto(&t, id).unwrap();
        assert_eq!(dto.name, "bad\u{fffd}name");
        serde_json::to_string(&dto).unwrap();
    }

    #[test]
    fn rect_dto_maps_aggregate_to_null_id() {
        use diskscape_core::category::Category;
        use diskscape_core::treemap::Rect;
        let rect = Rect { id: None, x: 1.0, y: 2.0, w: 3.0, h: 4.0, depth: 2, is_dir: false, category: Category::Archive };
        let json = serde_json::to_string(&RectDto::from(&rect)).unwrap();
        assert!(json.contains("\"id\":null"));
        assert!(json.contains("\"isDir\":false"));
        assert!(json.contains("\"category\":\"archive\""));
    }
}
