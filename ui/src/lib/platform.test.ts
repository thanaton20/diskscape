import { expect, test } from 'vitest';
import { detectPlatform } from './platform';

test('detectPlatform reads real webview user agents', () => {
  expect(detectPlatform('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15')).toBe('macos');
  expect(detectPlatform('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/120')).toBe('windows');
  expect(detectPlatform('Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15')).toBe('linux');
  // "darwin" contains "win": it must not be taken for Windows
  expect(detectPlatform('Mozilla/5.0 (darwin) AppleWebKit/537.36 jsdom/29')).toBe('linux');
  expect(detectPlatform('')).toBe('linux');
});
