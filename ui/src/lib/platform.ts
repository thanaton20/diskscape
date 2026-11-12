export type Platform = 'macos' | 'windows' | 'linux';

/** Which desktop OS the app runs on, from the browser's user agent. */
export function detectPlatform(userAgent: string): Platform {
  if (/Mac/i.test(userAgent)) return 'macos';
  if (/Win/i.test(userAgent)) return 'windows';
  return 'linux';
}

export const platform: Platform = detectPlatform(
  typeof navigator === 'undefined' ? '' : navigator.userAgent,
);
