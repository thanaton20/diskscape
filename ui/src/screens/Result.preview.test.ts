import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import Result from './Result.svelte';
import * as api from '../lib/api';
import { setLocale } from '../lib/i18n';
import { dir, mockLayout, node } from '../lib/testing';
import type { NodeDto, Summary } from '../lib/types';

vi.mock('../lib/api');
// Quick Look exists only on macOS.
vi.mock('../lib/platform', () => ({ platform: 'macos', detectPlatform: () => 'macos' }));

const KB = 1024;
const root = dir(0, '/r', 900 * KB, { items: 3 });
const files: NodeDto[] = [node(1, 'a.bin', 400 * KB), node(2, 'b.bin', 300 * KB), dir(3, 'sub', 200 * KB)];
const summary: Summary = {
  unreadableDirs: 0, unreadablePrivacy: 0, fullDiskAccess: false, totalDsize: 900 * KB, elapsedMs: 5000,
  readOnly: false, rootPath: '/r', treeId: 7,
};

let layout: ReturnType<typeof mockLayout>;

function mount(overrides: Partial<Summary> = {}) {
  vi.mocked(api.getSummary).mockResolvedValue({ ...summary, ...overrides });
  vi.mocked(api.getChildren).mockImplementation(async (id) => (id === 0 ? files : []));
  vi.mocked(api.getTreemap).mockResolvedValue([
    { id: 1, x: 0, y: 0, w: 400, h: 600, depth: 0, isDir: false, category: 'other', name: '' },
    { id: 2, x: 400, y: 0, w: 400, h: 600, depth: 0, isDir: false, category: 'other', name: '' },
  ]);
  vi.mocked(api.getAncestors).mockResolvedValue([]);
  vi.mocked(api.getNodes).mockImplementation(async (ids) =>
    ids.map((id) => (id === 0 ? root : (files.find((f) => f.id === id) ?? null))),
  );
  vi.mocked(api.getPath).mockImplementation(async (id) => `/r/${files.find((f) => f.id === id)?.name ?? id}`);
  vi.mocked(api.selectionInfo).mockImplementation(async (ids) => ({ count: ids.length, dsize: 1, hasSystemPath: false }));
  vi.mocked(api.togglePreview).mockResolvedValue(undefined);
  const onNewScan = vi.fn();
  render(Result, { props: { root, onNewScan } });
  return { onNewScan };
}

const row = (name: string) => screen.findByRole('row', { name: new RegExp(name) });
const map = () => screen.getByRole('img', { name: 'Map of disk usage' });

beforeEach(() => {
  vi.resetAllMocks();
  setLocale('en');
  layout = mockLayout(800, 600);
});
afterEach(() => layout.restore());

describe('Quick Look (macOS)', () => {
  test('Space previews the item selected in the list', async () => {
    mount();
    await userEvent.click(await row('a\\.bin'));
    await userEvent.keyboard(' ');
    await waitFor(() => expect(api.togglePreview).toHaveBeenCalledWith(1));
  });

  test('Space previews the item selected on the map', async () => {
    mount();
    await waitFor(() => expect(api.getTreemap).toHaveBeenCalled());
    await row('b\\.bin');
    await fireEvent.click(map(), { clientX: 600, clientY: 100 });
    await waitFor(async () => expect(await row('b\\.bin')).toHaveAttribute('aria-selected', 'true'));
    await userEvent.keyboard(' ');
    await waitFor(() => expect(api.togglePreview).toHaveBeenCalledWith(2));
  });

  test('Space does nothing without exactly one item selected', async () => {
    mount();
    await row('a\\.bin');
    await userEvent.keyboard(' ');
    const user = userEvent.setup();
    await user.click(await row('a\\.bin'));
    await user.keyboard('{Control>}');
    await user.click(await row('b\\.bin'));
    await user.keyboard('{/Control}');
    await user.keyboard(' ');
    expect(api.togglePreview).not.toHaveBeenCalled();
  });

  test('Space on a focused button presses the button instead', async () => {
    const { onNewScan } = mount();
    await userEvent.click(await row('a\\.bin'));
    screen.getByRole('button', { name: 'New scan' }).focus();
    await userEvent.keyboard(' ');
    expect(onNewScan).toHaveBeenCalledOnce();
    expect(api.togglePreview).not.toHaveBeenCalled();
  });

  test('a view opened from a file has no preview: its paths may not exist here', async () => {
    mount({ readOnly: true });
    await userEvent.click(await row('a\\.bin'));
    await screen.findByText(/view only/);
    await userEvent.keyboard(' ');
    expect(api.togglePreview).not.toHaveBeenCalled();
  });

  test('the context menu starts with Quick Look, for a single item', async () => {
    mount();
    await userEvent.pointer({ keys: '[MouseRight]', target: await row('a\\.bin') });
    const menu = await screen.findByRole('menu');
    const items = within(menu).getAllByRole('menuitem').map((i) => i.textContent?.trim());
    expect(items).toEqual(['Quick Look', 'Show in Finder', 'Copy path', 'Move to Trash', 'Delete permanently…']);
    await userEvent.click(within(menu).getByRole('menuitem', { name: 'Quick Look' }));
    await waitFor(() => expect(api.togglePreview).toHaveBeenCalledWith(1));
  });

  test('a failed preview is reported', async () => {
    mount();
    vi.mocked(api.togglePreview).mockRejectedValue('cannot start Quick Look');
    await userEvent.click(await row('a\\.bin'));
    await userEvent.keyboard(' ');
    expect(await screen.findByRole('alert')).toHaveTextContent('cannot start Quick Look');
  });
});
