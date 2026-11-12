import { describe, expect, test } from 'vitest';
import { hitTest } from './hitTest';
import type { RectDto } from './types';

const rect = (id: number | null, x: number, y: number, w: number, h: number, depth = 0, isDir = false): RectDto =>
  ({ id, x, y, w, h, depth, isDir, category: 'other' });

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
});
