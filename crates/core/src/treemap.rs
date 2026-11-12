//! Squarified treemap layout for one directory of the tree.
//!
//! The layout is computed here rather than in the interface so that only a
//! bounded number of rectangles ever crosses to the UI, however many files
//! the directory holds.

use crate::category::{categorize, Category};
use crate::tree::{NodeId, NodeKind, SortKey, Tree};
use std::collections::VecDeque;

/// One block of the map, in canvas pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Rect {
    /// `None` for the block that stands for all items too small to draw.
    pub id: Option<NodeId>,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Nesting level below the laid-out directory, starting at 0.
    pub depth: u8,
    pub is_dir: bool,
    pub category: Category,
}

/// Items whose block would be smaller than `MIN_PX` × `MIN_PX` are merged
/// into a single aggregate block.
pub const MIN_PX: f32 = 3.0;

/// Upper bound on the number of rectangles returned.
pub const MAX_RECTS: usize = 5000;

/// Upper bound on the blocks one folder may contribute, so that a single huge
/// folder cannot use up the whole budget and leave its siblings undrawn.
const MAX_PER_CONTAINER: usize = 2000;

/// Gap between a directory's frame and its contents.
const INSET: f64 = 1.0;

/// Lays out the contents of `dir` on a `width` × `height` canvas.
///
/// Rectangles come out breadth-first, so a directory always precedes its
/// contents and drawing them in order paints children over parents. Areas are
/// proportional to disk usage. Returns nothing for an empty or zero-sized
/// directory, a node that is not a directory, or a degenerate canvas.
pub fn layout(tree: &Tree, dir: NodeId, width: f32, height: f32) -> Vec<Rect> {
    let mut rects = Vec::new();
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return rects;
    }
    let mut queue: VecDeque<(NodeId, Area, u8)> = VecDeque::new();
    queue.push_back((dir, Area { x: 0.0, y: 0.0, w: f64::from(width), h: f64::from(height) }, 0));

    while let Some((container, area, depth)) = queue.pop_front() {
        // Children worth drawing, largest first.
        let kids: Vec<(NodeId, f64)> = tree
            .children_sorted(container, SortKey::Size, true)
            .into_iter()
            .filter_map(|id| {
                let n = tree.node(id)?;
                (n.dsize > 0 && !n.flags.hardlink_dup).then_some((id, n.dsize as f64))
            })
            .collect();
        let total: f64 = kids.iter().map(|(_, size)| size).sum();
        if total <= 0.0 || area.w <= 0.0 || area.h <= 0.0 {
            continue;
        }

        // Out of budget: this folder stays a plain frame rather than being
        // filled half-way.
        let allowed = MAX_RECTS.saturating_sub(rects.len()).min(MAX_PER_CONTAINER);
        if allowed == 0 {
            continue;
        }

        // Everything too small to see becomes one trailing aggregate block —
        // and so does whatever exceeds the budget. A folder is therefore always
        // tiled edge to edge; nothing is ever left blank.
        let scale = area.w * area.h / total;
        let min_area = f64::from(MIN_PX * MIN_PX);
        let mut visible = kids.partition_point(|(_, size)| size * scale >= min_area);
        let tail: f64 = kids[visible..].iter().map(|(_, size)| size).sum();
        let wanted = visible + usize::from(tail * scale >= min_area);
        let over_budget = wanted > allowed;
        if over_budget {
            visible = allowed - 1;
        }
        let rest: f64 = kids[visible..].iter().map(|(_, size)| size).sum();
        let mut items: Vec<(Option<NodeId>, f64)> =
            kids[..visible].iter().map(|(id, size)| (Some(*id), size * scale)).collect();
        if rest > 0.0 && (over_budget || rest * scale >= min_area) {
            // Keep the list sorted by area so the squarify rows stay well-shaped.
            let at = items.partition_point(|(_, a)| *a >= rest * scale);
            items.insert(at, (None, rest * scale));
        }

        let areas: Vec<f64> = items.iter().map(|(_, a)| *a).collect();
        for ((id, _), place) in items.iter().zip(squarify(&areas, area)) {
            if rects.len() >= MAX_RECTS {
                return rects;
            }
            let node = id.and_then(|id| tree.node(id));
            let is_dir = node.is_some_and(|n| n.kind == NodeKind::Dir);
            rects.push(Rect {
                id: *id,
                x: place.x as f32,
                y: place.y as f32,
                w: place.w as f32,
                h: place.h as f32,
                depth,
                is_dir,
                category: node.map_or(Category::Other, |n| categorize(&n.name)),
            });
            let roomy = place.w.min(place.h) >= f64::from(4.0 * MIN_PX);
            if let (Some(id), true, true) = (id, is_dir, roomy) {
                let inner = Area {
                    x: place.x + INSET,
                    y: place.y + INSET,
                    w: place.w - 2.0 * INSET,
                    h: place.h - 2.0 * INSET,
                };
                queue.push_back((*id, inner, depth.saturating_add(1)));
            }
        }
    }
    rects
}

#[derive(Clone, Copy, Debug)]
struct Area {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Squarified treemap (Bruls, Huizing, van Wijk): fills `bounds` with one
/// rectangle per entry of `areas` (largest first, summing to the area of
/// `bounds`), growing each row while that keeps its rectangles closer to square.
fn squarify(areas: &[f64], bounds: Area) -> Vec<Area> {
    /// Worst aspect ratio in a row of total area `sum` laid along a side of length `side`.
    fn worst(min: f64, max: f64, sum: f64, side: f64) -> f64 {
        let side2 = side * side;
        let sum2 = sum * sum;
        (side2 * max / sum2).max(sum2 / (side2 * min))
    }

    let mut out = Vec::with_capacity(areas.len());
    let mut free = bounds;
    let mut start = 0;
    while start < areas.len() {
        let side = free.w.min(free.h);
        if side <= 0.0 {
            // No room left (rounding); give the rest empty rectangles.
            out.extend(areas[start..].iter().map(|_| Area { x: free.x, y: free.y, w: 0.0, h: 0.0 }));
            break;
        }
        // Grow the row from `start` while the worst aspect ratio does not get worse.
        let mut end = start + 1;
        let (mut min, mut max, mut sum) = (areas[start], areas[start], areas[start]);
        while end < areas.len() {
            let a = areas[end];
            let (new_min, new_max, new_sum) = (min.min(a), max.max(a), sum + a);
            if worst(new_min, new_max, new_sum, side) > worst(min, max, sum, side) {
                break;
            }
            (min, max, sum) = (new_min, new_max, new_sum);
            end += 1;
        }

        // The row is a strip along the shorter side of the free space.
        let last_row = end == areas.len();
        if free.w >= free.h {
            let strip = if last_row { free.w } else { (sum / free.h).min(free.w) };
            let mut y = free.y;
            for a in &areas[start..end] {
                let h = a / sum * free.h;
                out.push(Area { x: free.x, y, w: strip, h });
                y += h;
            }
            free.x += strip;
            free.w -= strip;
        } else {
            let strip = if last_row { free.h } else { (sum / free.w).min(free.h) };
            let mut x = free.x;
            for a in &areas[start..end] {
                let w = a / sum * free.w;
                out.push(Area { x, y: free.y, w, h: strip });
                x += w;
            }
            free.y += strip;
            free.h -= strip;
        }
        start = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Entry, NodeId, NodeKind, Tree};
    use std::path::PathBuf;

    fn new_tree() -> Tree {
        Tree::new(PathBuf::from("/r"), Entry::new("/r", NodeKind::Dir, 0, 0), false)
    }

    fn file(t: &mut Tree, parent: NodeId, name: &str, dsize: u64) -> NodeId {
        t.add_child(parent, Entry::new(name, NodeKind::File, dsize, dsize))
    }

    fn dir(t: &mut Tree, parent: NodeId, name: &str) -> NodeId {
        t.add_child(parent, Entry::new(name, NodeKind::Dir, 0, 0))
    }

    fn area(r: &Rect) -> f32 {
        r.w * r.h
    }

    fn inside(inner: &Rect, outer: &Rect) -> bool {
        let e = 0.01;
        inner.x >= outer.x - e
            && inner.y >= outer.y - e
            && inner.x + inner.w <= outer.x + outer.w + e
            && inner.y + inner.h <= outer.y + outer.h + e
    }

    fn overlap(a: &Rect, b: &Rect) -> f32 {
        let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
        let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
        if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
    }

    fn all_finite(rects: &[Rect]) -> bool {
        rects.iter().all(|r| [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) && r.w >= 0.0 && r.h >= 0.0)
    }

    #[test]
    fn single_child_fills_the_canvas() {
        let mut t = new_tree();
        let root = t.root();
        let f = file(&mut t, root, "only", 4096);
        t.finalize();
        let r = layout(&t, root, 400.0, 300.0);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, Some(f));
        assert_eq!((r[0].x, r[0].y, r[0].w, r[0].h), (0.0, 0.0, 400.0, 300.0));
        assert!(!r[0].is_dir);
        assert_eq!(r[0].depth, 0);
    }

    #[test]
    fn leaf_areas_are_proportional_to_dsize() {
        let mut t = new_tree();
        let root = t.root();
        let big = file(&mut t, root, "big", 3000);
        let small = file(&mut t, root, "small", 1000);
        t.finalize();
        let r = layout(&t, root, 400.0, 300.0);
        assert_eq!(r.len(), 2);
        let a_big = area(r.iter().find(|x| x.id == Some(big)).unwrap());
        let a_small = area(r.iter().find(|x| x.id == Some(small)).unwrap());
        assert!((a_big / a_small - 3.0).abs() < 0.03, "ratio {}", a_big / a_small);
        assert!((a_big + a_small - 120_000.0).abs() < 600.0);
    }

    #[test]
    fn rects_stay_inside_bounds_and_siblings_do_not_overlap() {
        let mut t = new_tree();
        let root = t.root();
        for i in 0..40u64 {
            file(&mut t, root, &format!("f{i}"), 1000 + i * i * 700);
        }
        for d in 0..4u64 {
            let sub = dir(&mut t, root, &format!("d{d}"));
            for i in 0..10u64 {
                file(&mut t, sub, &format!("g{i}"), 20_000 + i * 9000 * (d + 1));
            }
        }
        t.finalize();
        let (w, h) = (800.0, 600.0);
        let r = layout(&t, root, w, h);
        assert!(all_finite(&r));
        let canvas = Rect { id: None, x: 0.0, y: 0.0, w, h, depth: 0, is_dir: true, category: crate::category::Category::Other };
        assert!(r.iter().all(|x| inside(x, &canvas)));

        let top: Vec<&Rect> = r.iter().filter(|x| x.depth == 0).collect();
        assert!(top.len() >= 40);
        for (i, a) in top.iter().enumerate() {
            for b in &top[i + 1..] {
                assert!(overlap(a, b) < 0.5, "top-level rects overlap");
            }
        }
        let total: f32 = top.iter().map(|x| area(x)).sum();
        assert!((total - w * h).abs() / (w * h) < 0.01, "top level covers {total}");

        let dirs: Vec<&&Rect> = top.iter().filter(|x| x.is_dir).collect();
        assert_eq!(dirs.len(), 4);
        let nested: Vec<&Rect> = r.iter().filter(|x| x.depth == 1).collect();
        assert!(!nested.is_empty());
        assert!(nested.iter().all(|n| dirs.iter().any(|d| inside(n, d))));
    }

    #[test]
    fn tiny_items_merge_into_one_aggregate() {
        let mut t = new_tree();
        let root = t.root();
        let big = file(&mut t, root, "big", 1_000_000);
        for i in 0..1000 {
            // 10 bytes of 1_010_000 on 480_000 px² is under 5 px² each: too small to draw
            file(&mut t, root, &format!("tiny{i}"), 10);
        }
        t.finalize();
        let r = layout(&t, root, 800.0, 600.0);
        assert_eq!(r.len(), 2);
        assert_eq!(r.iter().filter(|x| x.id.is_none()).count(), 1);
        assert!(r.iter().any(|x| x.id == Some(big)));
        let aggregate = r.iter().find(|x| x.id.is_none()).unwrap();
        // 10_000 of 1_010_000 bytes
        assert!((area(aggregate) / (800.0 * 600.0) - 1.0 / 101.0).abs() < 0.001);
    }

    #[test]
    fn never_more_than_max_rects() {
        let mut t = new_tree();
        let root = t.root();
        for i in 0..100_000 {
            file(&mut t, root, &format!("f{i}"), 4096);
        }
        t.finalize();
        let r = layout(&t, root, 4000.0, 3000.0);
        assert!(r.len() <= MAX_RECTS);
        assert!(r.len() > 1000);
        assert!(all_finite(&r));
    }

    #[test]
    fn large_folder_is_fully_tiled_within_the_budget() {
        // 20 000 equal files would each get a visible 35 px² block, far more than the budget.
        let mut t = new_tree();
        let root = t.root();
        for i in 0..20_000 {
            file(&mut t, root, &format!("f{i}"), 4096);
        }
        t.finalize();
        let (w, h) = (1000.0, 700.0);
        let r = layout(&t, root, w, h);
        assert!(r.len() <= MAX_RECTS);
        assert!(all_finite(&r));
        let covered: f32 = r.iter().map(area).sum();
        assert!(covered >= 0.99 * w * h, "only {covered} of {} px² drawn", w * h);
        // what did not fit is one "small items" block, not a blank
        assert_eq!(r.iter().filter(|x| x.id.is_none()).count(), 1);
    }

    #[test]
    fn nested_large_folders_are_tiled_or_left_as_plain_frames() {
        let mut t = new_tree();
        let root = t.root();
        for d in 0..3 {
            let sub = dir(&mut t, root, &format!("d{d}"));
            for i in 0..4000 {
                file(&mut t, sub, &format!("f{i}"), 4096);
            }
        }
        t.finalize();
        let (w, h) = (1000.0, 700.0);
        let r = layout(&t, root, w, h);
        assert!(r.len() <= MAX_RECTS);
        let top: Vec<&Rect> = r.iter().filter(|x| x.depth == 0).collect();
        assert_eq!(top.len(), 3);
        let covered: f32 = top.iter().map(|x| area(x)).sum();
        assert!(covered >= 0.99 * w * h);
        for folder in top {
            let inside: f32 = r.iter().filter(|x| x.depth == 1 && inside(x, folder)).map(area).sum();
            let interior = (folder.w - 2.0) * (folder.h - 2.0);
            // a folder is either filled edge to edge or not entered at all
            assert!(inside == 0.0 || inside >= 0.98 * interior, "{inside} of {interior} px² drawn");
        }
    }

    #[test]
    fn empty_dir_gives_no_rects() {
        let t = new_tree();
        assert!(layout(&t, t.root(), 800.0, 600.0).is_empty());
    }

    #[test]
    fn all_zero_sizes_give_no_rects_and_no_nan() {
        let mut t = new_tree();
        let root = t.root();
        for i in 0..10 {
            file(&mut t, root, &format!("z{i}"), 0);
        }
        let sub = dir(&mut t, root, "sub");
        file(&mut t, sub, "z", 0);
        t.finalize();
        let r = layout(&t, root, 800.0, 600.0);
        assert!(r.is_empty());
    }

    #[test]
    fn degenerate_canvas_gives_no_rects() {
        let mut t = new_tree();
        let root = t.root();
        file(&mut t, root, "f", 4096);
        t.finalize();
        for (w, h) in [(0.0, 600.0), (800.0, 0.0), (-5.0, 600.0), (f32::NAN, 600.0), (800.0, f32::INFINITY)] {
            assert!(layout(&t, root, w, h).is_empty(), "{w}x{h}");
        }
    }

    #[test]
    fn missing_or_file_node_gives_no_rects() {
        let mut t = new_tree();
        let root = t.root();
        let f = file(&mut t, root, "f", 4096);
        t.finalize();
        assert!(layout(&t, f, 800.0, 600.0).is_empty());
        assert!(layout(&t, 12345, 800.0, 600.0).is_empty());
    }

    #[test]
    fn nested_dirs_have_increasing_depth() {
        let mut t = new_tree();
        let root = t.root();
        let d1 = dir(&mut t, root, "d1");
        let d2 = dir(&mut t, d1, "d2");
        let f = file(&mut t, d2, "movie.mkv", 1_000_000);
        t.finalize();
        let r = layout(&t, root, 800.0, 600.0);
        let get = |id| r.iter().find(|x| x.id == Some(id)).unwrap();
        assert_eq!((get(d1).depth, get(d2).depth, get(f).depth), (0, 1, 2));
        assert!(get(d1).is_dir && get(d2).is_dir && !get(f).is_dir);
        assert!(inside(get(d2), get(d1)) && inside(get(f), get(d2)));
        assert!(area(get(f)) < area(get(d2)) && area(get(d2)) < area(get(d1)));
        assert_eq!(get(f).category, crate::category::Category::Video);
        // parents come before their children, so drawing in order works
        let pos = |id| r.iter().position(|x| x.id == Some(id)).unwrap();
        assert!(pos(d1) < pos(d2) && pos(d2) < pos(f));
    }

    #[test]
    fn hardlink_duplicates_are_not_drawn() {
        let mut t = new_tree();
        let root = t.root();
        for name in ["a", "b"] {
            let mut e = Entry::new(name, NodeKind::File, 4096, 4096);
            e.ino = 5;
            e.nlink = 2;
            t.add_child(root, e);
        }
        t.finalize();
        assert_eq!(layout(&t, root, 800.0, 600.0).len(), 1);
    }

    #[test]
    fn very_deep_nesting_terminates() {
        let mut t = new_tree();
        let mut parent = t.root();
        for i in 0..3000 {
            parent = dir(&mut t, parent, &format!("d{i}"));
        }
        file(&mut t, parent, "leaf", 4096);
        t.finalize();
        let r = layout(&t, t.root(), 800.0, 600.0);
        assert!(!r.is_empty() && r.len() <= MAX_RECTS);
        assert!(all_finite(&r));
    }
}
