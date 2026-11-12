import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import Result from './Result.svelte';
import * as api from '../lib/api';
import { setLocale } from '../lib/i18n';
import { dir, mockLayout, node } from '../lib/testing';
import type { DeleteResult, NodeDto, SelectionInfo, Summary } from '../lib/types';

vi.mock('../lib/api');

const KB = 1024;
const root = dir(0, '/r', 900 * KB, { items: 3 });
const files: NodeDto[] = [node(1, 'a.bin', 400 * KB), node(2, 'b.bin', 300 * KB), dir(3, 'sub', 200 * KB)];
const summary: Summary = {
  unreadableDirs: 0, unreadablePrivacy: 0, fullDiskAccess: false, totalDsize: 900 * KB, elapsedMs: 5000, readOnly: false, rootPath: '/r', treeId: 7,
};
const okResult = (id: number, freed: number): DeleteResult => ({ id, ok: true, freed, error: null });

let layout: ReturnType<typeof mockLayout>;
let writeText: ReturnType<typeof vi.fn>;

function mount(overrides: Partial<Summary> = {}) {
  vi.mocked(api.getSummary).mockResolvedValue({ ...summary, ...overrides });
  vi.mocked(api.getChildren).mockImplementation(async (id) => (id === 0 ? files : [node(9, 'inner.bin', 200 * KB)]));
  vi.mocked(api.getTreemap).mockResolvedValue([]);
  vi.mocked(api.getAncestors).mockResolvedValue([]);
  vi.mocked(api.getNodes).mockImplementation(async (ids) =>
    ids.map((id) => (id === 0 ? root : (files.find((f) => f.id === id) ?? null))),
  );
  vi.mocked(api.getPath).mockImplementation(async (id) => `/r/${files.find((f) => f.id === id)?.name ?? id}`);
  vi.mocked(api.selectionInfo).mockImplementation(async (ids) => ({
    count: ids.length,
    dsize: ids.reduce((sum, id) => sum + (files.find((f) => f.id === id)?.dsize ?? 0), 0),
    hasSystemPath: false,
  }));
  vi.mocked(api.deleteNodes).mockImplementation(async (ids) => ids.map((id) => okResult(id, 100 * KB)));
  vi.mocked(api.reveal).mockResolvedValue(undefined);
  render(Result, { props: { root, onNewScan: vi.fn() } });
}

const row = (name: string) => screen.findByRole('row', { name: new RegExp(name) });

async function openMenu(name: string) {
  await userEvent.pointer({ keys: '[MouseRight]', target: await row(name) });
  return screen.findByRole('menu');
}

beforeEach(() => {
  vi.resetAllMocks();
  setLocale('en');
  layout = mockLayout(800, 600);
  writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
});

afterEach(() => layout.restore());

describe('Result actions', () => {
  test('context menu has reveal, copy path, move to trash, delete permanently', async () => {
    mount();
    const menu = await openMenu('a\\.bin');
    const items = within(menu).getAllByRole('menuitem').map((i) => i.textContent?.trim());
    expect(items).toEqual([
      expect.stringMatching(/^Show in /), 'Copy path', 'Move to Trash', 'Delete permanently…',
    ]);
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  test('trash runs without a dialog and then refreshes the list, the map and the totals', async () => {
    mount();
    const menu = await openMenu('a\\.bin');
    await waitFor(() => expect(api.getTreemap).toHaveBeenCalledTimes(1));
    await userEvent.click(within(menu).getByRole('menuitem', { name: 'Move to Trash' }));
    await waitFor(() => expect(api.deleteNodes).toHaveBeenCalledWith([1], 'trash', 7));
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    await waitFor(() => expect(api.getTreemap).toHaveBeenCalledTimes(2));
    expect(api.getSummary).toHaveBeenCalledTimes(2);
    expect(await screen.findByText('Moved to Trash — items: 1, 100.0 KB')).toBeInTheDocument();
    // the selection is cleared afterwards
    await waitFor(() => expect(screen.queryByText(/^Selected:/)).not.toBeInTheDocument());
  });

  test('rows that no longer exist disappear after a delete', async () => {
    mount();
    const menu = await openMenu('b\\.bin');
    vi.mocked(api.getNodes).mockImplementation(async (ids) =>
      ids.map((id) => (id === 0 ? root : id === 2 ? null : (files.find((f) => f.id === id) ?? null))),
    );
    await userEvent.click(within(menu).getByRole('menuitem', { name: 'Move to Trash' }));
    await waitFor(() => expect(screen.queryByRole('row', { name: /b\.bin/ })).not.toBeInTheDocument());
    expect(screen.getByRole('row', { name: /a\.bin/ })).toBeInTheDocument();
  });

  test('permanent always opens the dialog with count and total size, and deletes only on confirm', async () => {
    mount();
    const user = userEvent.setup();
    await user.click(await row('a\\.bin'));
    await user.keyboard('{Control>}');
    await user.click(await row('b\\.bin'));
    await user.keyboard('{/Control}');
    await user.pointer({ keys: '[MouseRight]', target: await row('b\\.bin') });
    await user.click(await screen.findByRole('menuitem', { name: 'Delete permanently…' }));
    const dialog = await screen.findByRole('alertdialog');
    expect(dialog).toHaveTextContent('Items: 2, 700.0 KB');
    expect(dialog).toHaveTextContent('cannot be undone');
    expect(api.deleteNodes).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole('button', { name: 'Delete permanently' }));
    await waitFor(() => expect(api.deleteNodes).toHaveBeenCalledWith([1, 2], 'permanent', 7));
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    expect(await screen.findByText(/^Deleted permanently — items: 2/)).toBeInTheDocument();
  });

  test('the dialog deletes exactly what it described, even if the selection changes while it opens', async () => {
    mount();
    await userEvent.click(await row('a\\.bin'));
    let answer!: (info: SelectionInfo) => void;
    vi.mocked(api.selectionInfo).mockImplementationOnce(() => new Promise((resolve) => (answer = resolve)));
    await userEvent.keyboard('{Shift>}{Delete}{/Shift}');
    // the backend is slow; meanwhile the user clicks another row
    await userEvent.click(await row('b\\.bin'));
    answer({ count: 1, dsize: 400 * KB, hasSystemPath: false });
    const dialog = await screen.findByRole('alertdialog');
    expect(dialog).toHaveTextContent('/r/a.bin');
    expect(dialog).not.toHaveTextContent('/r/b.bin');
    await userEvent.click(within(dialog).getByRole('button', { name: 'Delete permanently' }));
    await waitFor(() => expect(api.deleteNodes).toHaveBeenCalledWith([1], 'permanent', 7));
    expect(api.deleteNodes).toHaveBeenCalledTimes(1);
  });

  test('the dialog names what will be deleted, up to five paths', async () => {
    mount();
    const user = userEvent.setup();
    await user.click(await row('a\\.bin'));
    await user.keyboard('{Shift>}');
    await user.click(await row('sub'));
    await user.keyboard('{/Shift}');
    await user.keyboard('{Shift>}{Delete}{/Shift}');
    const dialog = await screen.findByRole('alertdialog');
    for (const path of ['/r/a.bin', '/r/b.bin', '/r/sub']) expect(dialog).toHaveTextContent(path);
    expect(dialog).not.toHaveTextContent('more');
  });

  test('while a delete is running, further delete requests are ignored', async () => {
    mount();
    let finish!: (results: DeleteResult[]) => void;
    vi.mocked(api.deleteNodes).mockImplementation(() => new Promise((resolve) => (finish = resolve)));
    await userEvent.click(await row('a\\.bin'));
    await userEvent.keyboard('{Delete}');
    await waitFor(() => expect(api.deleteNodes).toHaveBeenCalledTimes(1));
    await userEvent.keyboard('{Delete}');
    await userEvent.keyboard('{Shift>}{Delete}{/Shift}');
    expect(api.deleteNodes).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    finish([okResult(1, 400 * KB)]);
    expect(await screen.findByText(/^Moved to Trash/)).toBeInTheDocument();
  });

  test('dialog focuses Cancel by default; Enter does not confirm; Escape cancels', async () => {
    mount();
    await userEvent.click(within(await openMenu('a\\.bin')).getByRole('menuitem', { name: 'Delete permanently…' }));
    const dialog = await screen.findByRole('alertdialog');
    await waitFor(() => expect(within(dialog).getByRole('button', { name: 'Cancel' })).toHaveFocus());
    await userEvent.keyboard('{Enter}');
    expect(api.deleteNodes).not.toHaveBeenCalled();
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();

    await userEvent.click(within(await openMenu('a\\.bin')).getByRole('menuitem', { name: 'Delete permanently…' }));
    await screen.findByRole('alertdialog');
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    expect(api.deleteNodes).not.toHaveBeenCalled();
  });

  test('dialog shows the system-path warning when the selection includes system files', async () => {
    mount();
    vi.mocked(api.selectionInfo).mockResolvedValue({ count: 1, dsize: 400 * KB, hasSystemPath: true });
    await userEvent.click(within(await openMenu('a\\.bin')).getByRole('menuitem', { name: 'Delete permanently…' }));
    expect(await screen.findByRole('alertdialog')).toHaveTextContent('system files');
  });

  test('no system warning for ordinary files', async () => {
    mount();
    await userEvent.click(within(await openMenu('a\\.bin')).getByRole('menuitem', { name: 'Delete permanently…' }));
    expect(await screen.findByRole('alertdialog')).not.toHaveTextContent('system files');
  });

  test('per-item errors are listed with path and reason', async () => {
    mount();
    vi.mocked(api.deleteNodes).mockResolvedValue([
      okResult(1, 400 * KB),
      { id: 2, ok: false, freed: 0, error: { kind: 'io', message: 'Permission denied (os error 13)' } },
      { id: 3, ok: false, freed: 0, error: { kind: 'refused', message: 'it has changed on disk since the scan' } },
    ]);
    const user = userEvent.setup();
    await user.click(await row('a\\.bin'));
    await user.keyboard('{Shift>}');
    await user.click(await row('sub'));
    await user.keyboard('{/Shift}');
    await user.keyboard('{Delete}');
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Items not deleted: 2');
    expect(alert).toHaveTextContent('/r/b.bin');
    expect(alert).toHaveTextContent('Permission denied');
    expect(alert).toHaveTextContent('/r/sub');
    expect(alert).toHaveTextContent('changed on disk since the scan');
    expect(await screen.findByText('Moved to Trash — items: 1, 400.0 KB')).toBeInTheDocument();
  });

  test('a trashUnavailable error never triggers a permanent delete', async () => {
    mount();
    vi.mocked(api.deleteNodes).mockResolvedValue([
      { id: 1, ok: false, freed: 0, error: { kind: 'trashUnavailable', message: 'no trash on this volume' } },
    ]);
    await userEvent.click(within(await openMenu('a\\.bin')).getByRole('menuitem', { name: 'Move to Trash' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Nothing was deleted');
    expect(api.deleteNodes).toHaveBeenCalledTimes(1);
    expect(api.deleteNodes).toHaveBeenCalledWith([1], 'trash', 7);
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
  });

  test('a partial failure is reported as such', async () => {
    mount();
    vi.mocked(api.deleteNodes).mockResolvedValue([
      { id: 3, ok: false, freed: 0, error: { kind: 'partial', message: 'Permission denied; part of it may already be deleted' } },
    ]);
    await userEvent.click(within(await openMenu('sub')).getByRole('menuitem', { name: 'Delete permanently…' }));
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: 'Delete permanently' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Stopped part-way');
  });

  test('a rejected delete request is shown and changes nothing', async () => {
    mount();
    vi.mocked(api.deleteNodes).mockRejectedValue('the list has changed since this selection was made; nothing was deleted');
    await userEvent.click(within(await openMenu('a\\.bin')).getByRole('menuitem', { name: 'Move to Trash' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('nothing was deleted');
  });

  test('keyboard: Delete moves to Trash, Shift+Delete asks first, and nothing happens without a selection', async () => {
    mount();
    await row('a\\.bin');
    await userEvent.keyboard('{Delete}');
    expect(api.deleteNodes).not.toHaveBeenCalled();
    await userEvent.click(await row('a\\.bin'));
    await userEvent.keyboard('{Shift>}{Delete}{/Shift}');
    expect(await screen.findByRole('alertdialog')).toBeInTheDocument();
    expect(api.deleteNodes).not.toHaveBeenCalled();
    await userEvent.keyboard('{Escape}');
    await userEvent.keyboard('{Delete}');
    await waitFor(() => expect(api.deleteNodes).toHaveBeenCalledWith([1], 'trash', 7));
  });

  test('shortcuts are ignored while the confirmation dialog is open', async () => {
    mount();
    await userEvent.click(await row('a\\.bin'));
    await userEvent.keyboard('{Shift>}{Delete}{/Shift}');
    await screen.findByRole('alertdialog');
    await userEvent.keyboard('{Delete}');
    expect(api.deleteNodes).not.toHaveBeenCalled();
  });

  test('a view opened from a file disables reveal, trash and permanent; copy and export stay enabled', async () => {
    mount({ readOnly: true });
    const menu = await openMenu('a\\.bin');
    await waitFor(() => expect(within(menu).getByRole('menuitem', { name: 'Move to Trash' })).toBeDisabled());
    expect(within(menu).getByRole('menuitem', { name: /^Show in / })).toBeDisabled();
    expect(within(menu).getByRole('menuitem', { name: 'Delete permanently…' })).toBeDisabled();
    expect(within(menu).getByRole('menuitem', { name: 'Copy path' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Export for ncdu…' })).toBeEnabled();
    await userEvent.keyboard('{Escape}');
    await userEvent.keyboard('{Delete}');
    await userEvent.keyboard('{Shift>}{Delete}{/Shift}');
    expect(api.deleteNodes).not.toHaveBeenCalled();
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    await userEvent.dblClick(await row('a\\.bin'));
    expect(api.reveal).not.toHaveBeenCalled();
  });

  test('reveal is offered for a single item only', async () => {
    mount();
    const user = userEvent.setup();
    await user.click(await row('a\\.bin'));
    await user.keyboard('{Control>}');
    await user.click(await row('b\\.bin'));
    await user.keyboard('{/Control}');
    await user.pointer({ keys: '[MouseRight]', target: await row('b\\.bin') });
    expect(await screen.findByRole('menuitem', { name: /^Show in / })).toBeDisabled();
  });

  test('double click on a file row reveals it', async () => {
    mount();
    await userEvent.dblClick(await row('a\\.bin'));
    await waitFor(() => expect(api.reveal).toHaveBeenCalledWith(1));
  });

  test('copy path puts the paths on the clipboard', async () => {
    mount();
    await userEvent.click(within(await openMenu('a\\.bin')).getByRole('menuitem', { name: 'Copy path' }));
    await waitFor(() => expect(api.copyText).toHaveBeenCalledWith('/r/a.bin'));
    expect(writeText).not.toHaveBeenCalled();
    expect(await screen.findByText('Path copied')).toBeInTheDocument();
  });

  test('export asks where to save and writes the file', async () => {
    mount();
    vi.mocked(api.pickSavePath).mockResolvedValueOnce(null);
    await userEvent.click(await screen.findByRole('button', { name: 'Export for ncdu…' }));
    expect(api.exportNcdu).not.toHaveBeenCalled();
    vi.mocked(api.pickSavePath).mockResolvedValueOnce('/tmp/out.json');
    vi.mocked(api.exportNcdu).mockResolvedValue(undefined);
    await userEvent.click(screen.getByRole('button', { name: 'Export for ncdu…' }));
    await waitFor(() => expect(api.exportNcdu).toHaveBeenCalledWith('/tmp/out.json'));
    expect(await screen.findByText('Exported to /tmp/out.json')).toBeInTheDocument();
  });
});
