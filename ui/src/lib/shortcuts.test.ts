import { describe, expect, test } from 'vitest';
import { matchShortcut } from './shortcuts';

const key = (
  k: string,
  mods: Partial<Record<'shiftKey' | 'altKey' | 'metaKey' | 'ctrlKey', boolean>> = {},
  code?: string,
) => ({ key: k, code, shiftKey: false, altKey: false, metaKey: false, ctrlKey: false, ...mods });

describe('matchShortcut', () => {
  test('Delete moves to the Trash on every platform', () => {
    expect(matchShortcut(key('Delete'), false)).toBe('trash');
    expect(matchShortcut(key('Delete'), true)).toBe('trash');
  });

  test('Shift+Delete deletes permanently', () => {
    expect(matchShortcut(key('Delete', { shiftKey: true }), false)).toBe('permanent');
    expect(matchShortcut(key('Delete', { shiftKey: true }), true)).toBe('permanent');
  });

  test('macOS Finder shortcuts', () => {
    expect(matchShortcut(key('Backspace', { metaKey: true }), true)).toBe('trash');
    expect(matchShortcut(key('Backspace', { metaKey: true, altKey: true }), true)).toBe('permanent');
    // not on other platforms, where Backspace is "go back"
    expect(matchShortcut(key('Backspace', { metaKey: true }), false)).toBeNull();
    expect(matchShortcut(key('Backspace', { ctrlKey: true }), false)).toBeNull();
  });

  test('copy path', () => {
    expect(matchShortcut(key('c', { ctrlKey: true }), false)).toBe('copyPath');
    expect(matchShortcut(key('C', { ctrlKey: true }), false)).toBe('copyPath');
    expect(matchShortcut(key('c', { metaKey: true }), true)).toBe('copyPath');
    expect(matchShortcut(key('c', { ctrlKey: true }), true)).toBeNull();
    expect(matchShortcut(key('c'), false)).toBeNull();
  });

  test('copy path works on a non-Latin keyboard layout', () => {
    // On a Russian layout the C key reports the Cyrillic letter "с".
    expect(matchShortcut(key('с', { ctrlKey: true }, 'KeyC'), false)).toBe('copyPath');
    expect(matchShortcut(key('с', { metaKey: true }, 'KeyC'), true)).toBe('copyPath');
    expect(matchShortcut(key('с', {}, 'KeyC'), false)).toBeNull();
    expect(matchShortcut(key('м', { ctrlKey: true }, 'KeyV'), false)).toBeNull();
  });

  test('Space asks for a preview, only without modifiers', () => {
    expect(matchShortcut(key(' '), true)).toBe('preview');
    expect(matchShortcut(key(' '), false)).toBe('preview');
    expect(matchShortcut(key(' ', { shiftKey: true }), true)).toBeNull();
    expect(matchShortcut(key(' ', { metaKey: true }), true)).toBeNull();
    expect(matchShortcut(key(' ', { ctrlKey: true }), false)).toBeNull();
  });

  test('anything else, and near misses, do nothing', () => {
    expect(matchShortcut(key('Backspace'), true)).toBeNull();
    expect(matchShortcut(key('Backspace'), false)).toBeNull();
    expect(matchShortcut(key('Delete', { ctrlKey: true }), false)).toBeNull();
    expect(matchShortcut(key('Delete', { altKey: true }), false)).toBeNull();
    expect(matchShortcut(key('Delete', { shiftKey: true, ctrlKey: true }), false)).toBeNull();
    expect(matchShortcut(key('Enter'), false)).toBeNull();
    expect(matchShortcut(key('d'), true)).toBeNull();
  });
});
