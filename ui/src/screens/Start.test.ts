import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import Start from './Start.svelte';
import * as api from '../lib/api';
import { setLocale } from '../lib/i18n';
import type { NodeDto } from '../lib/types';

vi.mock('../lib/api');

const root: NodeDto = {
  id: 0, name: '/sample', kind: 'dir', dsize: 1, asize: 1, items: 1, hasChildren: true,
  category: 'other', readError: false, childError: false, otherFs: false, hardlinkDup: false,
};

beforeEach(() => {
  vi.resetAllMocks();
  setLocale('en');
  vi.mocked(api.listVolumes).mockResolvedValue([
    { name: 'Macintosh HD', mountPoint: '/', total: 1000 * 1024 ** 3, free: 250 * 1024 ** 3 },
    { name: 'Backup', mountPoint: '/Volumes/Backup', total: 2 * 1024 ** 4, free: 1024 ** 4 },
  ]);
});

describe('Start', () => {
  test('lists volumes with a used/free bar and three entry points', async () => {
    render(Start, { props: { error: null, onScan: vi.fn(), onImported: vi.fn() } });
    expect(await screen.findByText('Macintosh HD')).toBeInTheDocument();
    expect(screen.getByText('Backup')).toBeInTheDocument();
    const bars = screen.getAllByRole('progressbar');
    expect(bars).toHaveLength(2);
    expect(bars[0]).toHaveAttribute('aria-valuenow', '75');
    expect(bars[1]).toHaveAttribute('aria-valuenow', '50');
    expect(screen.getByRole('button', { name: /choose folder/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /open ncdu file/i })).toBeInTheDocument();
  });

  test('clicking a volume scans its mount point', async () => {
    const onScan = vi.fn();
    render(Start, { props: { error: null, onScan, onImported: vi.fn() } });
    await userEvent.click(await screen.findByRole('button', { name: /Backup/ }));
    expect(onScan).toHaveBeenCalledWith('/Volumes/Backup');
  });

  test('choose folder scans the picked folder, and does nothing when the picker is dismissed', async () => {
    const onScan = vi.fn();
    render(Start, { props: { error: null, onScan, onImported: vi.fn() } });
    vi.mocked(api.pickFolder).mockResolvedValueOnce(null);
    await userEvent.click(screen.getByRole('button', { name: /choose folder/i }));
    expect(onScan).not.toHaveBeenCalled();
    vi.mocked(api.pickFolder).mockResolvedValueOnce('/Users/me/Downloads');
    await userEvent.click(screen.getByRole('button', { name: /choose folder/i }));
    await waitFor(() => expect(onScan).toHaveBeenCalledWith('/Users/me/Downloads'));
  });

  test('open ncdu file imports and reports the root', async () => {
    const onImported = vi.fn();
    render(Start, { props: { error: null, onScan: vi.fn(), onImported } });
    vi.mocked(api.pickNcduFile).mockResolvedValue('/tmp/export.json');
    vi.mocked(api.importNcdu).mockResolvedValue(root);
    await userEvent.click(screen.getByRole('button', { name: /open ncdu file/i }));
    await waitFor(() => expect(onImported).toHaveBeenCalledWith(root));
    expect(api.importNcdu).toHaveBeenCalledWith('/tmp/export.json');
  });

  test('a failed import shows the reason', async () => {
    render(Start, { props: { error: null, onScan: vi.fn(), onImported: vi.fn() } });
    vi.mocked(api.pickNcduFile).mockResolvedValue('/tmp/bad.json');
    vi.mocked(api.importNcdu).mockRejectedValue('not an ncdu export: missing format version');
    await userEvent.click(screen.getByRole('button', { name: /open ncdu file/i }));
    expect(await screen.findByRole('alert')).toHaveTextContent('missing format version');
  });

  test('shows an error passed in from a failed scan', async () => {
    render(Start, { props: { error: 'cannot read the scan root', onScan: vi.fn(), onImported: vi.fn() } });
    expect(await screen.findByRole('alert')).toHaveTextContent('cannot read the scan root');
  });
});
