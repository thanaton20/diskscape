import { beforeEach, describe, expect, test, vi } from 'vitest';
import * as api from './api';
import { dir, node } from './testing';
import { TreeModel } from './treeModel.svelte';
import type { NodeDto } from './types';

vi.mock('./api');

// /r (0) -> a (1, 600), sub (2, 400) -> x (3, 300), deep (4, 100) -> leaf (5, 100)
const kids: Record<number, NodeDto[]> = {
  0: [node(1, 'a', 600), dir(2, 'sub', 400)],
  2: [node(3, 'x', 300), dir(4, 'deep', 100)],
  4: [node(5, 'leaf', 100)],
};
const root = dir(0, '/r', 1000);
const ids = (m: TreeModel) => m.rows.map((r) => r.node.id);

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.getChildren).mockImplementation(async (id) => kids[id] ?? []);
  vi.mocked(api.getAncestors).mockImplementation(async (id) =>
    ({ 5: [root, kids[0][1], kids[2][1]], 3: [root, kids[0][1]], 1: [root] })[id] ?? [],
  );
});

describe('TreeModel', () => {
  test('starts with the root only', () => {
    const m = new TreeModel(root);
    expect(ids(m)).toEqual([0]);
    expect(m.rows[0]).toMatchObject({ depth: 0, expanded: false });
  });

  test('expand inserts children after the row with depth + 1 and their share of the parent', async () => {
    const m = new TreeModel(root);
    await m.expand(0);
    expect(ids(m)).toEqual([0, 1, 2]);
    expect(m.rows[0].expanded).toBe(true);
    expect(m.rows.slice(1).map((r) => r.depth)).toEqual([1, 1]);
    expect(m.rows.slice(1).map((r) => r.share)).toEqual([0.6, 0.4]);
    await m.expand(2);
    expect(ids(m)).toEqual([0, 1, 2, 3, 4]);
    expect(m.rows[3]).toMatchObject({ depth: 2, share: 0.75 });
    expect(api.getChildren).toHaveBeenCalledWith(2, 'size', true);
  });

  test('expanding twice, a file, or an unknown id changes nothing', async () => {
    const m = new TreeModel(root);
    await m.expand(0);
    await m.expand(0);
    await m.expand(1);
    await m.expand(999);
    expect(ids(m)).toEqual([0, 1, 2]);
    expect(api.getChildren).toHaveBeenCalledTimes(1);
  });

  test('collapse removes all descendants', async () => {
    const m = new TreeModel(root);
    await m.expand(0);
    await m.expand(2);
    await m.expand(4);
    expect(ids(m)).toEqual([0, 1, 2, 3, 4, 5]);
    m.collapse(2);
    expect(ids(m)).toEqual([0, 1, 2]);
    expect(m.rows[2].expanded).toBe(false);
    m.collapse(0);
    expect(ids(m)).toEqual([0]);
  });

  test('revealNode expands every ancestor and returns the row index', async () => {
    const m = new TreeModel(root);
    expect(await m.revealNode(5)).toBe(5);
    expect(ids(m)).toEqual([0, 1, 2, 3, 4, 5]);
    expect(await m.revealNode(1)).toBe(1);
    expect(await m.revealNode(12345)).toBe(-1);
  });

  test('refresh drops removed nodes with their descendants and updates sizes and shares', async () => {
    const m = new TreeModel(root);
    await m.revealNode(5);
    // `deep` (4) and its leaf were deleted; sub shrank to 300 and root to 900.
    vi.mocked(api.getNodes).mockResolvedValue([
      dir(0, '/r', 900), node(1, 'a', 600), dir(2, 'sub', 300), node(3, 'x', 300), null, null,
    ]);
    await m.refresh();
    expect(ids(m)).toEqual([0, 1, 2, 3]);
    expect(m.rows[0].node.dsize).toBe(900);
    expect(m.rows[2].node.dsize).toBe(300);
    expect(m.rows[2].share).toBeCloseTo(300 / 900);
    expect(m.rows[3].share).toBe(1);
    expect(api.getNodes).toHaveBeenCalledWith([0, 1, 2, 3, 4, 5]);
  });

  test('refresh collapses a folder that has become empty', async () => {
    const m = new TreeModel(root);
    await m.expand(0);
    await m.expand(2);
    vi.mocked(api.getNodes).mockResolvedValue([
      dir(0, '/r', 600), node(1, 'a', 600), dir(2, 'sub', 0, { hasChildren: false }), null, null,
    ]);
    await m.refresh();
    expect(ids(m)).toEqual([0, 1, 2]);
    expect(m.rows[2].expanded).toBe(false);
  });

  test('setSort reloads expanded folders in the new order', async () => {
    const m = new TreeModel(root);
    await m.expand(0);
    await m.expand(2);
    vi.mocked(api.getChildren).mockImplementation(async (id, sort) => {
      const list = [...(kids[id] ?? [])];
      return sort === 'name' ? list.reverse() : list;
    });
    await m.setSort('name', false);
    expect(ids(m)).toEqual([0, 2, 4, 3, 1]);
    expect(api.getChildren).toHaveBeenLastCalledWith(2, 'name', false);
    expect(m.sort).toBe('name');
    expect(m.desc).toBe(false);
  });
});
