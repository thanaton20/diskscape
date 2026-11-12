import type { Category } from './types';

export type Theme = 'light' | 'dark';

/** Hue and saturation per file category. */
const TONES: Record<Category, [number, number]> = {
  video: [265, 58],
  audio: [325, 58],
  image: [150, 48],
  archive: [35, 78],
  document: [208, 68],
  code: [182, 46],
  app: [2, 64],
  other: [225, 12],
};

/** Fill colour of a file block. Nesting depth nudges the shade so neighbouring levels differ. */
export function categoryColor(category: Category, theme: Theme, depth: number): string {
  const [hue, saturation] = TONES[category];
  const step = (depth % 4) * 3;
  const lightness = theme === 'light' ? 64 - step : 44 + step;
  return `hsl(${hue} ${saturation}% ${lightness}%)`;
}

/** Fill colour of a folder frame, behind its contents. */
export function folderColor(theme: Theme, depth: number): string {
  const step = (depth % 4) * 3;
  return theme === 'light' ? `hsl(225 18% ${90 - step}%)` : `hsl(225 14% ${17 + step}%)`;
}

/** Colour of the gaps between blocks. */
export function gapColor(theme: Theme): string {
  return theme === 'light' ? 'hsl(225 20% 97%)' : 'hsl(225 16% 9%)';
}
