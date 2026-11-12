import { describe, expect, test } from 'vitest';
import { formatCount, formatDuration, formatSize } from './format';

describe('formatSize', () => {
  test('binary units with one decimal', () => {
    expect(formatSize(0, 'en')).toBe('0 B');
    expect(formatSize(1023, 'en')).toBe('1023 B');
    expect(formatSize(1024, 'en')).toBe('1.0 KB');
    expect(formatSize(1536, 'en')).toBe('1.5 KB');
    expect(formatSize(1610612736, 'en')).toBe('1.5 GB');
    expect(formatSize(1024 ** 4 * 3, 'en')).toBe('3.0 TB');
  });

  test('russian units and decimal comma', () => {
    expect(formatSize(0, 'ru')).toBe('0 Б');
    expect(formatSize(1610612736, 'ru')).toBe('1,5 ГБ');
    expect(formatSize(5 * 1024 * 1024, 'ru')).toBe('5,0 МБ');
  });

  test('never switches unit at a rounding edge', () => {
    // 1023.99 KB must not print as "1024.0 KB"
    expect(formatSize(1024 * 1024 - 1, 'en')).toBe('1.0 MB');
  });

  test('bad input does not print NaN', () => {
    expect(formatSize(Number.NaN, 'en')).toBe('0 B');
    expect(formatSize(-5, 'en')).toBe('0 B');
  });
});

describe('formatCount and formatDuration', () => {
  test('thousands separators', () => {
    expect(formatCount(1234567, 'en')).toBe('1,234,567');
    expect(formatCount(1234567, 'ru').replace(/\s/g, ' ')).toBe('1 234 567');
    expect(formatCount(0, 'en')).toBe('0');
  });

  test('durations', () => {
    expect(formatDuration(900, 'en')).toBe('0.9 s');
    expect(formatDuration(12_340, 'en')).toBe('12.3 s');
    expect(formatDuration(65_000, 'en')).toBe('1 min 05 s');
    expect(formatDuration(12_340, 'ru')).toBe('12,3 с');
    expect(formatDuration(65_000, 'ru')).toBe('1 мин 05 с');
  });
});
