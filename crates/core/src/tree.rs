//! In-memory result tree: nodes, aggregated totals, sorting and removal.
//!
//! Every traversal here is iterative: real directory trees can be thousands
//! of levels deep and must not overflow the stack.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;

/// Index of a node inside its [`Tree`].
pub type NodeId = u32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    Dir,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Flags {
    /// This directory could not be read, so its totals are incomplete.
    pub read_error: bool,
    /// Something below this node could not be read.
    pub child_error: bool,
    /// Lives on another filesystem and was not descended into.
    pub other_fs: bool,
    /// Another name of an inode that is already counted elsewhere in the tree.
    pub hardlink_dup: bool,
}

/// What a scanner or importer knows about one filesystem object.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: OsString,
    pub kind: NodeKind,
    /// Apparent size in bytes.
    pub asize: u64,
    /// Size occupied on disk in bytes.
    pub dsize: u64,
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    pub read_error: bool,
    pub other_fs: bool,
}

impl Entry {
    pub fn new(name: impl Into<OsString>, kind: NodeKind, asize: u64, dsize: u64) -> Entry {
        Entry {
            name: name.into(),
            kind,
            asize,
            dsize,
            dev: 0,
            ino: 0,
            nlink: 1,
            read_error: false,
            other_fs: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: OsString,
    pub parent: Option<NodeId>,
    pub kind: NodeKind,
    /// Size of the object itself, as reported by the filesystem.
    pub own_asize: u64,
    pub own_dsize: u64,
    /// Totals including all descendants; valid after [`Tree::finalize`].
    pub asize: u64,
    pub dsize: u64,
    /// Number of descendants; valid after [`Tree::finalize`].
    pub items: u64,
    pub flags: Flags,
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    pub children: Vec<NodeId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Size,
    Name,
    Items,
}

pub struct Tree {
    nodes: Vec<Option<Node>>,
    root: NodeId,
    root_path: PathBuf,
    read_only: bool,
    unreadable: u64,
}

impl Tree {
    /// `root_path` is where the root lives on disk; `root.name` is only its label.
    pub fn new(root_path: PathBuf, root: Entry, read_only: bool) -> Tree {
        let mut tree = Tree {
            nodes: Vec::new(),
            root: 0,
            root_path,
            read_only,
            unreadable: 0,
        };
        tree.root = tree.push(None, root);
        tree
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Imported trees are view-only: nothing in them may be deleted or revealed.
    pub fn read_only(&self) -> bool {
        self.read_only
    }

    /// Adds `e` under `parent`. Totals are stale until [`Tree::finalize`] runs.
    pub fn add_child(&mut self, parent: NodeId, e: Entry) -> NodeId {
        let id = self.push(Some(parent), e);
        if let Some(Some(p)) = self.nodes.get_mut(parent as usize) {
            p.children.push(id);
        }
        id
    }

    fn push(&mut self, parent: Option<NodeId>, e: Entry) -> NodeId {
        let id = NodeId::try_from(self.nodes.len()).expect("more than u32::MAX nodes");
        self.nodes.push(Some(Node {
            name: e.name,
            parent,
            kind: e.kind,
            own_asize: e.asize,
            own_dsize: e.dsize,
            asize: e.asize,
            dsize: e.dsize,
            items: 0,
            flags: Flags {
                read_error: e.read_error,
                child_error: false,
                other_fs: e.other_fs,
                hardlink_dup: false,
            },
            dev: e.dev,
            ino: e.ino,
            nlink: e.nlink,
            children: Vec::new(),
        }));
        id
    }

    /// Marks a directory as unreadable. Totals are stale until [`Tree::finalize`] runs.
    pub fn set_read_error(&mut self, id: NodeId) {
        if let Some(Some(node)) = self.nodes.get_mut(id as usize) {
            node.flags.read_error = true;
        }
    }

    /// `None` once the node has been removed.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id as usize).and_then(Option::as_ref)
    }

    /// Recomputes totals, item counts, error propagation and hardlink
    /// de-duplication for the whole tree. Safe to call repeatedly.
    ///
    /// Of several names of one inode, the first in name-ordered pre-order
    /// carries the size; the others are flagged `hardlink_dup` and add nothing
    /// to any total. This keeps the result independent of scan order.
    pub fn finalize(&mut self) {
        let mut order: Vec<NodeId> = Vec::with_capacity(self.nodes.len());
        let mut seen: HashSet<(u64, u64)> = HashSet::new();
        let mut unreadable = 0u64;
        let mut stack = vec![self.root];

        while let Some(id) = stack.pop() {
            let Some(mut children) = self.nodes[id as usize]
                .as_mut()
                .map(|n| std::mem::take(&mut n.children))
            else {
                continue;
            };
            children.sort_by(|a, b| self.name_of(*a).cmp(self.name_of(*b)));
            stack.extend(children.iter().rev().copied());

            let node = self.nodes[id as usize].as_mut().expect("checked above");
            node.children = children;
            node.items = 0;
            node.flags.child_error = false;
            node.flags.hardlink_dup = node.kind != NodeKind::Dir
                && node.nlink > 1
                && !seen.insert((node.dev, node.ino));
            if node.flags.hardlink_dup {
                node.asize = 0;
                node.dsize = 0;
            } else {
                node.asize = node.own_asize;
                node.dsize = node.own_dsize;
            }
            if node.flags.read_error {
                unreadable += 1;
            }
            order.push(id);
        }

        // Reverse pre-order visits every node after all of its descendants.
        for &id in order.iter().rev() {
            let node = self.nodes[id as usize].as_ref().expect("visited above");
            let Some(parent) = node.parent else { continue };
            let (asize, dsize, items) = (node.asize, node.dsize, node.items);
            let error = node.flags.read_error || node.flags.child_error;
            if let Some(Some(p)) = self.nodes.get_mut(parent as usize) {
                p.asize = p.asize.saturating_add(asize);
                p.dsize = p.dsize.saturating_add(dsize);
                p.items += items + 1;
                p.flags.child_error |= error;
            }
        }
        self.unreadable = unreadable;
    }

    fn name_of(&self, id: NodeId) -> &std::ffi::OsStr {
        self.node(id).map(|n| n.name.as_os_str()).unwrap_or_default()
    }

    /// Children of `id` ordered by `key`; ties are always broken by name ascending.
    pub fn children_sorted(&self, id: NodeId, key: SortKey, desc: bool) -> Vec<NodeId> {
        let Some(node) = self.node(id) else {
            return Vec::new();
        };
        let mut kids = node.children.clone();
        kids.sort_by(|a, b| self.name_of(*a).cmp(self.name_of(*b)));
        let metric = |id: &NodeId| -> u64 {
            let n = self.node(*id);
            match key {
                SortKey::Size => n.map_or(0, |n| n.dsize),
                SortKey::Items => n.map_or(0, |n| n.items),
                SortKey::Name => 0,
            }
        };
        match (key, desc) {
            (SortKey::Name, false) => {}
            (SortKey::Name, true) => kids.reverse(),
            (_, false) => kids.sort_by_key(metric),
            (_, true) => kids.sort_by_key(|id| std::cmp::Reverse(metric(id))),
        }
        kids
    }

    /// Full path of the node on disk.
    pub fn path(&self, id: NodeId) -> PathBuf {
        let mut names = Vec::new();
        let mut cur = id;
        while let Some(node) = self.node(cur) {
            let Some(parent) = node.parent else { break };
            names.push(node.name.as_os_str());
            cur = parent;
        }
        let mut path = self.root_path.clone();
        path.extend(names.into_iter().rev());
        path
    }

    /// Ancestors from the root down to the direct parent; empty for the root.
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = self.node(id).and_then(|n| n.parent);
        while let Some(p) = cur {
            out.push(p);
            cur = self.node(p).and_then(|n| n.parent);
        }
        out.reverse();
        out
    }

    /// Removes the node and everything below it, then recomputes totals.
    /// Returns how much disk space the tree's total shrank by — which is zero
    /// when the removed file has another hard link left in the tree.
    /// Removing the root empties it but keeps the root node itself.
    pub fn remove(&mut self, id: NodeId) -> u64 {
        let Some(node) = self.node(id) else { return 0 };
        let before = self.node(self.root).map_or(0, |n| n.dsize);

        let doomed: Vec<NodeId> = match node.parent {
            Some(parent) => {
                if let Some(Some(p)) = self.nodes.get_mut(parent as usize) {
                    p.children.retain(|c| *c != id);
                }
                vec![id]
            }
            None => {
                let root = self.nodes[id as usize].as_mut().expect("checked above");
                root.own_asize = 0;
                root.own_dsize = 0;
                std::mem::take(&mut root.children)
            }
        };

        let mut stack = doomed;
        while let Some(cur) = stack.pop() {
            if let Some(gone) = self.nodes.get_mut(cur as usize).and_then(Option::take) {
                stack.extend(gone.children);
            }
        }

        self.finalize();
        let after = self.node(self.root).map_or(0, |n| n.dsize);
        before.saturating_sub(after)
    }

    /// Number of directories that could not be read.
    pub fn unreadable_dirs(&self) -> u64 {
        self.unreadable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn file(name: &str, asize: u64, dsize: u64) -> Entry {
        Entry::new(name, NodeKind::File, asize, dsize)
    }

    fn dir(name: &str) -> Entry {
        Entry::new(name, NodeKind::Dir, 0, 0)
    }

    fn link(name: &str, dev: u64, ino: u64) -> Entry {
        let mut e = file(name, 5, 4096);
        e.dev = dev;
        e.ino = ino;
        e.nlink = 2;
        e
    }

    /// `/r/{a.txt 5/4096, sub/{b.bin 2/4096}}`
    fn fixture() -> (Tree, NodeId, NodeId, NodeId) {
        let mut t = Tree::new(PathBuf::from("/r"), dir("/r"), false);
        let a = t.add_child(t.root(), file("a.txt", 5, 4096));
        let sub = t.add_child(t.root(), dir("sub"));
        let b = t.add_child(sub, file("b.bin", 2, 4096));
        t.finalize();
        (t, a, sub, b)
    }

    /// `/r/{a.txt, sub/{hard.txt}}`, both names are links to the same inode.
    fn hardlink_fixture() -> (Tree, NodeId, NodeId, NodeId) {
        let mut t = Tree::new(PathBuf::from("/r"), dir("/r"), false);
        let sub = t.add_child(t.root(), dir("sub"));
        let hard = t.add_child(sub, link("hard.txt", 1, 7));
        let a = t.add_child(t.root(), link("a.txt", 1, 7));
        t.finalize();
        (t, a, sub, hard)
    }

    #[test]
    fn finalize_sums_sizes_and_items() {
        let (t, _, sub, _) = fixture();
        let root = t.node(t.root()).unwrap();
        assert_eq!(root.dsize, 8192);
        assert_eq!(root.asize, 7);
        assert_eq!(root.items, 3);
        let sub = t.node(sub).unwrap();
        assert_eq!(sub.dsize, 4096);
        assert_eq!(sub.items, 1);
    }

    #[test]
    fn finalize_is_idempotent() {
        let (mut t, _, _, _) = fixture();
        t.finalize();
        t.finalize();
        assert_eq!(t.node(t.root()).unwrap().dsize, 8192);
        assert_eq!(t.node(t.root()).unwrap().items, 3);
    }

    #[test]
    fn hardlinks_counted_once_first_by_name_carries() {
        let (t, a, sub, hard) = hardlink_fixture();
        assert_eq!(t.node(t.root()).unwrap().dsize, 4096);
        assert_eq!(t.node(t.root()).unwrap().asize, 5);
        assert!(!t.node(a).unwrap().flags.hardlink_dup);
        assert!(t.node(hard).unwrap().flags.hardlink_dup);
        assert_eq!(t.node(sub).unwrap().dsize, 0);
        // the duplicate still reports its own size for display
        assert_eq!(t.node(hard).unwrap().own_dsize, 4096);
    }

    #[test]
    fn read_error_propagates_child_error_to_ancestors() {
        let mut t = Tree::new(PathBuf::from("/r"), dir("/r"), false);
        let sub = t.add_child(t.root(), dir("sub"));
        let mut locked = dir("locked");
        locked.read_error = true;
        let locked = t.add_child(sub, locked);
        t.finalize();
        assert!(t.node(locked).unwrap().flags.read_error);
        assert!(!t.node(locked).unwrap().flags.child_error);
        assert!(t.node(sub).unwrap().flags.child_error);
        assert!(t.node(t.root()).unwrap().flags.child_error);
        assert_eq!(t.unreadable_dirs(), 1);
    }

    #[test]
    fn children_sorted_by_size_desc_then_name() {
        let mut t = Tree::new(PathBuf::from("/r"), dir("/r"), false);
        let small_b = t.add_child(t.root(), file("small_b", 1, 512));
        let big = t.add_child(t.root(), file("big", 1, 9000));
        let small_a = t.add_child(t.root(), file("small_a", 1, 512));
        t.finalize();
        assert_eq!(
            t.children_sorted(t.root(), SortKey::Size, true),
            vec![big, small_a, small_b]
        );
        assert_eq!(
            t.children_sorted(t.root(), SortKey::Name, false),
            vec![big, small_a, small_b]
        );
        assert_eq!(
            t.children_sorted(t.root(), SortKey::Name, true),
            vec![small_b, small_a, big]
        );
    }

    #[test]
    fn children_sorted_by_items() {
        let mut t = Tree::new(PathBuf::from("/r"), dir("/r"), false);
        let few = t.add_child(t.root(), dir("few"));
        let many = t.add_child(t.root(), dir("many"));
        t.add_child(few, file("x", 1, 1));
        t.add_child(many, file("x", 1, 1));
        t.add_child(many, file("y", 1, 1));
        t.finalize();
        assert_eq!(
            t.children_sorted(t.root(), SortKey::Items, true),
            vec![many, few]
        );
    }

    #[test]
    fn remove_subtracts_from_all_ancestors() {
        let (mut t, _, sub, b) = fixture();
        assert_eq!(t.remove(sub), 4096);
        let root = t.node(t.root()).unwrap();
        assert_eq!(root.dsize, 4096);
        assert_eq!(root.asize, 5);
        assert_eq!(root.items, 1);
        assert!(t.node(sub).is_none());
        assert!(t.node(b).is_none());
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn remove_twice_is_a_no_op() {
        let (mut t, a, _, _) = fixture();
        assert_eq!(t.remove(a), 4096);
        assert_eq!(t.remove(a), 0);
        assert_eq!(t.node(t.root()).unwrap().dsize, 4096);
    }

    #[test]
    fn remove_hardlink_carrier_moves_size_to_other_link() {
        let (mut t, a, sub, hard) = hardlink_fixture();
        assert_eq!(t.remove(a), 0);
        assert_eq!(t.node(t.root()).unwrap().dsize, 4096);
        assert!(!t.node(hard).unwrap().flags.hardlink_dup);
        assert_eq!(t.node(sub).unwrap().dsize, 4096);
    }

    #[test]
    fn remove_last_hardlink_frees_the_size() {
        let (mut t, a, _, hard) = hardlink_fixture();
        assert_eq!(t.remove(hard), 0);
        assert_eq!(t.remove(a), 4096);
        assert_eq!(t.node(t.root()).unwrap().dsize, 0);
    }

    #[test]
    fn remove_root_empties_the_tree_but_keeps_the_root() {
        let (mut t, _, _, _) = fixture();
        let root = t.root();
        assert_eq!(t.remove(root), 8192);
        let root = t.node(root).unwrap();
        assert_eq!(root.dsize, 0);
        assert_eq!(root.items, 0);
        assert!(root.children.is_empty());
    }

    #[test]
    fn finalize_handles_depth_5000_without_stack_overflow() {
        let mut t = Tree::new(PathBuf::from("/r"), dir("/r"), false);
        let mut parent = t.root();
        for i in 0..5000 {
            parent = t.add_child(parent, dir(&format!("d{i}")));
        }
        let leaf = t.add_child(parent, file("leaf", 3, 512));
        t.finalize();
        assert_eq!(t.node(t.root()).unwrap().dsize, 512);
        assert_eq!(t.node(t.root()).unwrap().items, 5001);
        assert_eq!(t.ancestors(leaf).len(), 5001);
        assert_eq!(t.path(leaf).components().count(), 5003);
        assert_eq!(t.remove(leaf), 512);
    }

    #[test]
    fn path_joins_root_path_and_names() {
        let (t, _, _, b) = fixture();
        assert_eq!(t.path(b), PathBuf::from("/r/sub/b.bin"));
        assert_eq!(t.path(t.root()), PathBuf::from("/r"));
    }

    #[test]
    fn ancestors_go_from_root_to_parent() {
        let (t, _, sub, b) = fixture();
        assert_eq!(t.ancestors(b), vec![t.root(), sub]);
        assert!(t.ancestors(t.root()).is_empty());
    }

    #[test]
    fn read_only_flag_is_kept() {
        let t = Tree::new(PathBuf::from("/r"), dir("/r"), true);
        assert!(t.read_only());
    }
}
