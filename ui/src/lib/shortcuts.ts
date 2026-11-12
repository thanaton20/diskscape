export type Action = 'trash' | 'permanent' | 'copyPath';

type Keys = Pick<KeyboardEvent, 'key' | 'shiftKey' | 'altKey' | 'metaKey' | 'ctrlKey'> & { code?: string };

/**
 * Which action a key press asks for, if any.
 *
 * - Delete → Trash; on macOS also ⌘⌫, as in Finder.
 * - Shift+Delete → delete permanently; on macOS also ⌥⌘⌫.
 * - Ctrl+C (⌘C on macOS) → copy path.
 *
 * Any extra modifier makes it "no action": a near miss must never delete.
 */
export function matchShortcut(e: Keys, isMac: boolean): Action | null {
  const mods = [e.shiftKey, e.altKey, e.metaKey, e.ctrlKey];
  const only = (...held: boolean[]) => mods.every((down, i) => down === held[i]);

  if (e.key === 'Delete') {
    if (only(false, false, false, false)) return 'trash';
    if (only(true, false, false, false)) return 'permanent';
    return null;
  }
  if (e.key === 'Backspace' && isMac) {
    if (only(false, false, true, false)) return 'trash';
    if (only(false, true, true, false)) return 'permanent';
    return null;
  }
  // The letter C — or, on a layout with no Latin letters (Russian reports "с"),
  // the key in C's position.
  const letter = e.key.toLowerCase();
  if (letter === 'c' || (!/^[a-z]$/.test(letter) && e.code === 'KeyC')) {
    return (isMac ? only(false, false, true, false) : only(false, false, false, true)) ? 'copyPath' : null;
  }
  return null;
}
