import type { Locale } from './i18n';

const UNITS: Record<Locale, string[]> = {
  en: ['B', 'KB', 'MB', 'GB', 'TB', 'PB'],
  ru: ['Б', 'КБ', 'МБ', 'ГБ', 'ТБ', 'ПБ'],
};

const decimal = (text: string, locale: Locale) => (locale === 'ru' ? text.replace('.', ',') : text);

/** Size in binary units with one decimal: `1.5 GB`, `1,5 ГБ`. */
export function formatSize(bytes: number, locale: Locale): string {
  const units = UNITS[locale];
  if (!Number.isFinite(bytes) || bytes <= 0) return `0 ${units[0]}`;
  if (bytes < 1024) return `${Math.round(bytes)} ${units[0]}`;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  let text = value.toFixed(1);
  // 1023.99 KB rounds to "1024.0": show it as 1.0 of the next unit instead.
  if (text === '1024.0' && unit < units.length - 1) {
    text = '1.0';
    unit += 1;
  }
  return `${decimal(text, locale)} ${units[unit]}`;
}

/** Whole number with thousands separators. */
export function formatCount(n: number, locale: Locale): string {
  return new Intl.NumberFormat(locale === 'ru' ? 'ru-RU' : 'en-US').format(Math.max(0, Math.round(n)));
}

/** `12.3 s` under a minute, `1 min 05 s` above. */
export function formatDuration(ms: number, locale: Locale): string {
  const [min, sec] = locale === 'ru' ? ['мин', 'с'] : ['min', 's'];
  const safe = Number.isFinite(ms) && ms > 0 ? ms : 0;
  if (safe < 60_000) return `${decimal((safe / 1000).toFixed(1), locale)} ${sec}`;
  const total = Math.floor(safe / 1000);
  return `${Math.floor(total / 60)} ${min} ${String(total % 60).padStart(2, '0')} ${sec}`;
}
