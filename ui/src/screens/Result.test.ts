import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import Result from './Result.svelte';
import * as api from '../lib/api';
import { setLocale } from '../lib/i18n';
import { dir, mockLayout, node } from '../lib/testing';
import type { Summary } from '../lib/types';

vi.mock('../lib/api');

const root = dir(0, '/r', 1024 * 1024, { items: 2 });
const summary: Summary = {
  unreadableDirs: 0, unreadablePrivacy: 0, fullDiskAccess: false, totalDsize: 1024 * 1024, elapsedMs: 12_300, readOnly: false, rootPath: '/r', treeId: 7,
};

function mount(overrides: Partial<Summary> = {}, prepare: () => void = () => {}) {
  vi.mocked(api.getSummary).mockResolvedValue({ ...summary, ...overrides });
  vi.mocked(api.getChildren).mockResolvedValue([node(1, 'a.bin', 1024 * 512), node(2, 'b.bin', 1024 * 512)]);
  vi.mocked(api.getTreemap).mockResolvedValue([]);
  vi.mocked(api.getAncestors).mockResolvedValue([]);
  vi.mocked(api.selectionInfo).mockImplementation(async (ids) => ({
    count: ids.length, dsize: ids.length * 1024 * 512, hasSystemPath: false,
  }));
  vi.mocked(api.getNodes).mockResolvedValue([root]);
  prepare();
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

  describe('with the map', () => {
    let layout: ReturnType<typeof mockLayout>;
    beforeEach(() => {
      layout = mockLayout(800, 600);
    });
    afterEach(() => layout.restore());

    // /r -> a.bin (1), b.bin (2), sub (3) -> inner.bin (9)
    const mountWithMap = () => mount({}, () => {
      vi.mocked(api.getChildren).mockImplementation(async (id) =>
        id === 0
          ? [node(1, 'a.bin', 400), node(2, 'b.bin', 300), dir(3, 'sub', 300)]
          : [node(9, 'inner.bin', 300)],
      );
      vi.mocked(api.getAncestors).mockImplementation(async (id) =>
        id === 9 ? [root, dir(3, 'sub', 300)] : id === 3 ? [root] : [],
      );
      vi.mocked(api.getNodes).mockImplementation(async (ids) =>
        ids.map((id) => (id === 3 ? dir(3, 'sub', 300) : id === 0 ? root : node(id, `n${id}`, 300))),
      );
      vi.mocked(api.getPath).mockResolvedValue('/r/x');
      vi.mocked(api.getTreemap).mockResolvedValue([
        { id: 3, x: 0, y: 0, w: 400, h: 600, depth: 0, isDir: true, category: 'other', name: '' },
        { id: 1, x: 400, y: 0, w: 400, h: 600, depth: 0, isDir: false, category: 'other', name: '' },
        { id: 9, x: 10, y: 10, w: 100, h: 100, depth: 1, isDir: false, category: 'other', name: '' },
      ]);
    });

    const map = () => screen.getByRole('img', { name: 'Map of disk usage' });

    test('selecting on the map reveals and selects the row in the list', async () => {
      mountWithMap();
      await waitFor(() => expect(api.getTreemap).toHaveBeenCalledWith(0, 800, 600));
      expect(screen.queryByRole('row', { name: /inner\.bin/ })).not.toBeInTheDocument();
      await fireEvent.click(map(), { clientX: 50, clientY: 50 });
      const row = await screen.findByRole('row', { name: /inner\.bin/ });
      await waitFor(() => expect(row).toHaveAttribute('aria-selected', 'true'));
    });

    test('selecting in the list highlights the rect on the map', async () => {
      mountWithMap();
      await userEvent.click(await screen.findByRole('row', { name: /a\.bin/ }));
      await waitFor(() => expect(screen.getAllByTestId('map-selection')).toHaveLength(1));
      expect(screen.getByTestId('map-selection').style.left).toBe('400px');
    });

    test('double click on a folder in the map enters it, and the breadcrumbs lead back', async () => {
      mountWithMap();
      await waitFor(() => expect(api.getTreemap).toHaveBeenCalledWith(0, 800, 600));
      await fireEvent.dblClick(map(), { clientX: 300, clientY: 400 });
      await waitFor(() => expect(api.getTreemap).toHaveBeenLastCalledWith(3, 800, 600));
      await userEvent.click(await screen.findByRole('button', { name: '/r' }));
      await waitFor(() => expect(api.getTreemap).toHaveBeenLastCalledWith(0, 800, 600));
    });
  });
});
