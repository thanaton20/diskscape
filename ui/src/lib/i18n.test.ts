import { describe, expect, test } from 'vitest';
import { detectLocale, setLocale, t } from './i18n';
import en from './locales/en.json';
import ru from './locales/ru.json';

describe('i18n', () => {
  test('detectLocale', () => {
    expect(detectLocale('ru-RU')).toBe('ru');
    expect(detectLocale('ru')).toBe('ru');
    expect(detectLocale('de')).toBe('en');
    expect(detectLocale('en-GB')).toBe('en');
    expect(detectLocale('')).toBe('en');
  });

  test('t substitutes params and falls back to the key', () => {
    setLocale('en');
    expect(t('banner.unreadable', { n: 3 })).toContain('3');
    expect(t('banner.unreadable', { n: 3 })).not.toContain('{n}');
    expect(t('no.such.key')).toBe('no.such.key');
    setLocale('ru');
    expect(t('progress.cancel')).toBe('Отмена');
    expect(t('banner.unreadable', { n: 12 })).toContain('12');
    setLocale('en');
    expect(t('progress.cancel')).toBe('Cancel');
  });

  test('en and ru have identical key sets and no empty strings', () => {
    expect(Object.keys(ru).sort()).toEqual(Object.keys(en).sort());
    for (const dict of [en, ru]) {
      for (const [key, value] of Object.entries(dict)) {
        expect(value, key).not.toBe('');
      }
    }
  });

  test('both languages use the same placeholders', () => {
    const holes = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort();
    for (const key of Object.keys(en) as (keyof typeof en)[]) {
      expect(holes(ru[key]), key).toEqual(holes(en[key]));
    }
  });
});
