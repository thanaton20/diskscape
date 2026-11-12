// The tree list as a flat array of visible rows. The whole tree lives in the
// backend; this holds only what is expanded on screen.
import * as api from './api';
import type { NodeDto, SortKey } from './types';

export interface Row {
  node: NodeDto;
  depth: number;
  expanded: boolean;
  /** This node's disk usage as a fraction of its parent's, 0..1. */
  share: number;
}

export class TreeModel {
  // Replaced wholesale on every change (never mutated in place): a folder can
  // hold hundreds of thousands of entries, and deep reactivity on each is wasted.
  rows = $state.raw<Row[]>([]);
  sort = $state<SortKey>('size');
  desc = $state(true);

  constructor(root: NodeDto) {
    this.rows = [{ node: root, depth: 0, expanded: false, share: 1 }];
  }

  indexOf(id: number): number {
    return this.rows.findIndex((r) => r.node.id === id);
  }

  /** Loads and shows the children of a folder. No-op for files and open folders. */
  async expand(id: number): Promise<void> {
    const before = this.rows[this.indexOf(id)];
    if (!before || before.expanded || !before.node.hasChildren) return;
    const children = await api.getChildren(id, this.sort, this.desc);
    // The list may have changed while the request was in flight.
    const index = this.indexOf(id);
    const row = this.rows[index];
    if (!row || row.expanded) return;
    const depth = row.depth + 1;
    const parentSize = row.node.dsize;
    const added: Row[] = children.map((node) => ({
      node, depth, expanded: false, share: fraction(node.dsize, parentSize),
    }));
    // concat, not splice(...added): spreading a huge folder would overflow the call stack.
    this.rows = this.rows
      .slice(0, index)
      .concat({ ...row, expanded: true }, added, this.rows.slice(index + 1));
  }

  /** Hides everything below a folder. */
  collapse(id: number): void {
    const index = this.indexOf(id);
    const row = this.rows[index];
    if (!row || !row.expanded) return;
    let end = index + 1;
    while (end < this.rows.length && this.rows[end].depth > row.depth) end += 1;
    this.rows = this.rows
      .slice(0, index)
      .concat({ ...row, expanded: false }, this.rows.slice(end));
  }

  /** Expands every ancestor of a node so its row exists; returns the row index or -1. */
  async revealNode(id: number): Promise<number> {
    if (this.indexOf(id) < 0) {
      for (const ancestor of await api.getAncestors(id)) {
        await this.expand(ancestor.id);
      }
    }
    return this.indexOf(id);
  }

  /** Re-reads every visible node after a change: removed nodes disappear, sizes update. */
  async refresh(): Promise<void> {
    const current = this.rows.map((r) => ({ ...r }));
    const fresh = await api.getNodes(current.map((r) => r.node.id));
    const kept: Row[] = [];
    const parentSizes: number[] = [];
    let skipBelow: number | null = null;
    current.forEach((row, i) => {
      if (skipBelow !== null && row.depth > skipBelow) return;
      skipBelow = null;
      const node = fresh[i];
      if (!node) {
        skipBelow = row.depth;
        return;
      }
      parentSizes[row.depth] = node.dsize;
      kept.push({
        node,
        depth: row.depth,
        expanded: row.expanded && node.hasChildren,
        share: row.depth === 0 ? 1 : fraction(node.dsize, parentSizes[row.depth - 1] ?? 0),
      });
      // A folder that lost all its children has nothing left to show below it.
      if (row.expanded && !node.hasChildren) skipBelow = row.depth;
    });
    this.rows = kept;
  }

  /** Changes the order and reloads every open folder in it. */
  async setSort(sort: SortKey, desc: boolean): Promise<void> {
    this.sort = sort;
    this.desc = desc;
    const open = this.rows.filter((r) => r.expanded).map((r) => r.node.id);
    const root = this.rows[0];
    this.rows = [{ ...root, expanded: false }];
    // Rows are in pre-order, so a parent is always reopened before its children.
    for (const id of open) {
      await this.expand(id);
    }
  }
}

function fraction(part: number, whole: number): number {
  return whole > 0 ? Math.min(1, part / whole) : 0;
}
