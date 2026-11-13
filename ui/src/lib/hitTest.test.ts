import { describe, expect, test } from 'vitest';
import { hitTest, hitTestFolder } from './hitTest';
import type { RectDto } from './types';

const rect = (id: number | null, x: number, y: number, w: number, h: number, depth = 0, isDir = false): RectDto =>
  ({ id, x, y, w, h, depth, isDir, category: 'other', name: '' });

// A directory (1) covering the left half with a file (2) inside it, and a file (3) on the right.
const rects = [rect(1, 0, 0, 100, 100, 0, true), rect(3, 100, 0, 100, 100), rect(2, 10, 10, 50, 50, 1)];

describe('hitTest', () => {
  test('returns the deepest rect containing the point', () => {
    expect(hitTest(rects, 20, 20)?.id).toBe(2);
    expect(hitTest(rects, 80, 80)?.id).toBe(1);
    expect(hitTest(rects, 150, 50)?.id).toBe(3);
  });

  test('returns null outside every rect', () => {
    expect(hitTest(rects, 250, 50)).toBeNull();
    expect(hitTest(rects, -1, 50)).toBeNull();
    expect(hitTest([], 10, 10)).toBeNull();
  });

  test('edges belong to the rect on the left/top side only', () => {
    expect(hitTest(rects, 100, 50)?.id).toBe(3);
    expect(hitTest(rects, 0, 0)?.id).toBe(1);
  });

  test('hitTestFolder returns the innermost folder under the point', () => {
    // inside file 2, which lies in folder 1: the folder that holds the block is what opens
    expect(hitTestFolder(rects, 20, 20)?.id).toBe(1);
    expect(hitTestFolder(rects, 80, 80)?.id).toBe(1);
    // a top-level file has no folder below the one already shown
    expect(hitTestFolder(rects, 150, 50)).toBeNull();
    expect(hitTestFolder(rects, 250, 50)).toBeNull();
    // folder 4 sits inside folder 1, file 6 inside folder 4
    const nested = [rect(1, 0, 0, 100, 100, 0, true), rect(4, 5, 5, 60, 60, 1, true), rect(6, 6, 6, 20, 20, 2)];
    expect(hitTestFolder(nested, 10, 10)?.id).toBe(4);
    expect(hitTestFolder(nested, 50, 50)?.id).toBe(4);
    expect(hitTestFolder(nested, 80, 80)?.id).toBe(1);
  });
});
