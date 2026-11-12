import type { RectDto } from './types';

/**
 * The deepest block under a point, or `null`. Blocks arrive parents-first,
 * so the last one containing the point is the innermost.
 */
export function hitTest(rects: RectDto[], x: number, y: number): RectDto | null {
  for (let i = rects.length - 1; i >= 0; i -= 1) {
    const r = rects[i];
    if (x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) return r;
  }
  return null;
}
