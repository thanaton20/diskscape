export type Platform = 'macos' | 'windows' | 'linux';

/** Which desktop OS the app runs on, from the webview's user agent. */
export function detectPlatform(userAgent: string): Platform {
  if (/Macintosh|Mac OS X/.test(userAgent)) return 'macos';
  if (/Windows/.test(userAgent)) return 'windows';
  return 'linux';
}

export const platform: Platform = detectPlatform(
  typeof navigator === 'undefined' ? '' : navigator.userAgent,
);
