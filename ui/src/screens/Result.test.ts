import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import Result from './Result.svelte';
import * as api from '../lib/api';
import { setLocale } from '../lib/i18n';
import { dir, node } from '../lib/testing';
import type { Summary } from '../lib/types';

vi.mock('../lib/api');

const root = dir(0, '/r', 1024 * 1024, { items: 2 });
const summary: Summary = {
  unreadableDirs: 0, totalDsize: 1024 * 1024, elapsedMs: 12_300, readOnly: false, rootPath: '/r', treeId: 7,
};

function mount(overrides: Partial<Summary> = {}) {
  vi.mocked(api.getSummary).mockResolvedValue({ ...summary, ...overrides });
  vi.mocked(api.getChildren).mockResolvedValue([node(1, 'a.bin', 1024 * 512), node(2, 'b.bin', 1024 * 512)]);
  vi.mocked(api.getTreemap).mockResolvedValue([]);
  vi.mocked(api.getAncestors).mockResolvedValue([]);
  vi.mocked(api.selectionInfo).mockImplementation(async (ids) => ({
    count: ids.length, dsize: ids.length * 1024 * 512, hasSystemPath: false,
  }));
  const onNewScan = vi.fn();
  render(Result, { props: { root, onNewScan } });
  return { onNewScan };
}

beforeEach(() => {
  vi.resetAllMocks();
  setLocale('en');
});

describe('Result', () => {
  test('shows the tree with the root expanded and the status bar', async () => {
    mount();
    expect(await screen.findByRole('row', { name: /a\.bin/ })).toBeInTheDocument();
    expect(await screen.findByText('Total: 1.0 MB')).toBeInTheDocument();
    expect(screen.getByText('Scanned in 12.3 s')).toBeInTheDocument();
  });

  test('status bar shows the size of the selection', async () => {
    mount();
    await userEvent.click(await screen.findByRole('row', { name: /a\.bin/ }));
    expect(await screen.findByText('Selected: 512.0 KB')).toBeInTheDocument();
  });

  test('banner appears only when folders could not be read, and its button asks for access', async () => {
    mount({ unreadableDirs: 3 });
    const banner = await screen.findByRole('status');
    expect(banner).toHaveTextContent('3');
    vi.mocked(api.requestElevation).mockResolvedValue(undefined);
    await userEvent.click(screen.getByRole('button', { name: /Full Disk Access|administrator/ }));
    expect(api.requestElevation).toHaveBeenCalledOnce();
  });

  test('no banner when everything was read', async () => {
    mount();
    await screen.findByRole('row', { name: /a\.bin/ });
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  test('a view opened from a file says so', async () => {
    mount({ readOnly: true });
    expect(await screen.findByText(/view only/)).toBeInTheDocument();
  });

  test('new scan goes back', async () => {
    const { onNewScan } = mount();
    await userEvent.click(await screen.findByRole('button', { name: 'New scan' }));
    expect(onNewScan).toHaveBeenCalledOnce();
  });

  test('the divider can be moved with the keyboard and stays within 20–80%', async () => {
    mount();
    const divider = await screen.findByRole('separator');
    expect(divider).toHaveAttribute('aria-valuenow', '45');
    divider.focus();
    await userEvent.keyboard('{ArrowRight}');
    await waitFor(() => expect(divider).toHaveAttribute('aria-valuenow', '47'));
    for (let i = 0; i < 40; i++) await userEvent.keyboard('{ArrowRight}');
    expect(divider).toHaveAttribute('aria-valuenow', '80');
    for (let i = 0; i < 60; i++) await userEvent.keyboard('{ArrowLeft}');
    expect(divider).toHaveAttribute('aria-valuenow', '20');
  });
});
