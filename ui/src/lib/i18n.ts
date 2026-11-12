import en from './locales/en.json';
import ru from './locales/ru.json';

export type Locale = 'en' | 'ru';

const dictionaries: Record<Locale, Record<string, string>> = { en, ru };

/** Russian for any `ru*` language tag, English otherwise. */
export function detectLocale(lang: string): Locale {
  return lang.toLowerCase().startsWith('ru') ? 'ru' : 'en';
}

let current: Locale = detectLocale(typeof navigator === 'undefined' ? '' : navigator.language);

export function setLocale(locale: Locale): void {
  current = locale;
}

export function getLocale(): Locale {
  return current;
}

/** Translated text for `key`, with `{name}` placeholders filled from `params`. */
export function t(key: string, params: Record<string, string | number> = {}): string {
  const template = dictionaries[current][key] ?? dictionaries.en[key] ?? key;
  return template.replace(/\{(\w+)\}/g, (hole, name: string) =>
    name in params ? String(params[name]) : hole,
  );
}
