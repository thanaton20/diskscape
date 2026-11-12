import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import UnreadableBanner from './UnreadableBanner.svelte';
import * as api from '../lib/api';
import { setLocale } from '../lib/i18n';

vi.mock('../lib/api');

beforeEach(() => {
  vi.resetAllMocks();
  setLocale('en');
});

describe('UnreadableBanner', () => {
  test('macOS: folders closed by ordinary permissions get no Full Disk Access button', () => {
    // e.g. /private/var/root: owned by the system; Full Disk Access does not open it
    render(UnreadableBanner, { props: { count: 14, privacy: 0, fullAccess: false, os: 'macos' } });
    const banner = screen.getByRole('status');
    expect(banner).toHaveTextContent('14');
    expect(banner).toHaveTextContent('Full Disk Access does not open them');
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  test('macOS: folders blocked by privacy protection offer Full Disk Access', async () => {
    vi.mocked(api.requestElevation).mockResolvedValue(undefined);
    render(UnreadableBanner, { props: { count: 20, privacy: 6, fullAccess: false, os: 'macos' } });
    const banner = screen.getByRole('status');
    expect(banner).toHaveTextContent('20');
    expect(banner).toHaveTextContent('blocks 6');
    expect(banner).not.toHaveTextContent('does not open them');
    await userEvent.click(screen.getByRole('button', { name: 'Open Full Disk Access settings' }));
    expect(api.requestElevation).toHaveBeenCalledOnce();
  });

  test('macOS: once Full Disk Access is granted it is never offered again', () => {
    // Even with it, macOS keeps some of its own folders closed ("operation not permitted").
    render(UnreadableBanner, { props: { count: 207, privacy: 57, fullAccess: true, os: 'macos' } });
    const banner = screen.getByRole('status');
    expect(banner).toHaveTextContent('207');
    expect(banner).toHaveTextContent('Full Disk Access is already granted');
    expect(banner).not.toHaveTextContent('Grant Full Disk Access');
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  test('Windows and Linux always offer a restart with administrator rights', () => {
    for (const [os, label] of [['windows', 'Restart as administrator'], ['linux', 'Restart with administrator rights']] as const) {
      const view = render(UnreadableBanner, { props: { count: 3, privacy: 0, fullAccess: false, os } });
      expect(screen.getByRole('button', { name: label })).toBeInTheDocument();
      view.unmount();
    }
  });

  test('a failed request is shown', async () => {
    vi.mocked(api.requestElevation).mockRejectedValue('the request for administrator rights was declined');
    render(UnreadableBanner, { props: { count: 3, privacy: 0, fullAccess: false, os: 'windows' } });
    await userEvent.click(screen.getByRole('button'));
    expect(await screen.findByText(/was declined/)).toBeInTheDocument();
  });
});
