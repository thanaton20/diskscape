import { expect, test } from 'vitest';
import { categoryColor } from './palette';
import type { Category } from './types';

const all: Category[] = ['video', 'audio', 'image', 'archive', 'document', 'code', 'app', 'other'];

test('categoryColor differs per category and per theme, and is stable', () => {
  for (const theme of ['light', 'dark'] as const) {
    const colors = all.map((c) => categoryColor(c, theme, 0));
    expect(new Set(colors).size).toBe(all.length);
    colors.forEach((c) => expect(c).toMatch(/^hsl\(/));
  }
  expect(categoryColor('video', 'light', 0)).not.toBe(categoryColor('video', 'dark', 0));
  expect(categoryColor('video', 'light', 0)).toBe(categoryColor('video', 'light', 0));
  // nesting shifts the shade a little so neighbouring levels can be told apart
  expect(categoryColor('video', 'light', 1)).not.toBe(categoryColor('video', 'light', 0));
});
