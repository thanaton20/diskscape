import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import TreeList from './TreeList.svelte';
import * as api from '../lib/api';
import { setLocale } from '../lib/i18n';
import { dir, node } from '../lib/testing';
import { TreeModel } from '../lib/treeModel.svelte';
import type { NodeDto } from '../lib/types';

vi.mock('../lib/api');

const root = dir(0, '/r', 1000, { items: 4 });
const children: NodeDto[] = [
  node(1, 'movie.mkv', 600),
  dir(2, 'sub', 300, { items: 12345, childError: true }),
  node(3, 'link', 60, { kind: 'symlink' }),
  dir(4, 'locked', 30, { readError: true, hasChildren: false }),
  dir(5, 'mnt', 10, { otherFs: true, hasChildren: false }),
];

async function setup(selected: number[] = []) {
  vi.mocked(api.getChildren).mockImplementation(async (id) => (id === 0 ? children : [node(9, 'inner', 300)]));
  const model = new TreeModel(root);
  await model.expand(0);
  const handlers = { onSelect: vi.fn(), onOpen: vi.fn(), onContextMenu: vi.fn() };
  render(TreeList, { props: { model, selected, ...handlers } });
  return { model, ...handlers };
}

const row = (name: string) => screen.getByRole('row', { name: new RegExp(name) });

beforeEach(() => {
  vi.resetAllMocks();
  setLocale('en');
});

describe('TreeList', () => {
  test('row shows name, size, share bar and item count', async () => {
    await setup();
    const sub = row('sub');
    expect(within(sub).getByText('300 B')).toBeInTheDocument();
    expect(within(sub).getByText('12,345')).toBeInTheDocument();
    expect(within(sub).getByRole('meter')).toHaveAttribute('aria-valuenow', '30');
    expect(row('movie.mkv')).toHaveAttribute('aria-level', '2');
    expect(row('/r')).toHaveAttribute('aria-expanded', 'true');
  });

  test('rows show icons for readError, childError, symlink and otherFs', async () => {
    await setup();
    expect(within(row('locked')).getByTitle('Could not be read')).toBeInTheDocument();
    expect(within(row('sub')).getByTitle(/folders that could not be read/)).toBeInTheDocument();
    expect(within(row('link')).getByTitle('Symbolic link')).toBeInTheDocument();
    expect(within(row('mnt')).getByTitle(/Another filesystem/)).toBeInTheDocument();
  });

  test('renders only visible rows for 100 000 rows', async () => {
    const many = Array.from({ length: 100_000 }, (_, i) => node(i + 1, `f${i}`, 1));
    vi.mocked(api.getChildren).mockResolvedValue(many);
    const model = new TreeModel(root);
    await model.expand(0);
    render(TreeList, { props: { model, selected: [], onSelect: vi.fn(), onOpen: vi.fn(), onContextMenu: vi.fn() } });
    const rows = screen.getAllByRole('row');
    expect(rows.length).toBeGreaterThan(5);
    expect(rows.length).toBeLessThan(100);
    expect(screen.getByRole('treegrid')).toHaveAttribute('aria-rowcount', '100001');
  });

  test('click selects, ctrl/cmd-click toggles, shift-click selects a range', async () => {
    const user = userEvent.setup();
    const { onSelect } = await setup([1]);
    await user.click(row('sub'));
    expect(onSelect).toHaveBeenLastCalledWith([2]);
    await user.keyboard('{Control>}');
    await user.click(row('link'));
    await user.keyboard('{/Control}');
    expect(onSelect).toHaveBeenLastCalledWith([1, 3]);
    await user.keyboard('{Control>}');
    await user.click(row('movie.mkv'));
    await user.keyboard('{/Control}');
    expect(onSelect).toHaveBeenLastCalledWith([]);
  });

  test('shift-click selects from the last clicked row', async () => {
    const user = userEvent.setup();
    const { onSelect } = await setup();
    await user.click(row('movie.mkv'));
    await user.keyboard('{Shift>}');
    await user.click(row('locked'));
    await user.keyboard('{/Shift}');
    expect(onSelect).toHaveBeenLastCalledWith([1, 2, 3, 4]);
  });

  test('selected rows are marked', async () => {
    await setup([2, 3]);
    expect(row('sub')).toHaveAttribute('aria-selected', 'true');
    expect(row('link')).toHaveAttribute('aria-selected', 'true');
    expect(row('movie.mkv')).toHaveAttribute('aria-selected', 'false');
  });

  test('the disclosure button expands and collapses without changing the selection', async () => {
    const user = userEvent.setup();
    const { model, onSelect } = await setup();
    await user.click(within(row('sub')).getByRole('button', { name: 'Expand' }));
    expect(await screen.findByRole('row', { name: /inner/ })).toBeInTheDocument();
    expect(onSelect).not.toHaveBeenCalled();
    await user.click(within(row('sub')).getByRole('button', { name: 'Collapse' }));
    expect(model.rows.some((r) => r.node.id === 9)).toBe(false);
  });

  test('double click opens a file and toggles a folder', async () => {
    const user = userEvent.setup();
    const { onOpen } = await setup();
    await user.dblClick(row('movie.mkv'));
    expect(onOpen).toHaveBeenCalledWith(1);
    await user.dblClick(row('sub'));
    expect(await screen.findByRole('row', { name: /inner/ })).toBeInTheDocument();
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  test('right click selects the row if needed and asks for a menu', async () => {
    const user = userEvent.setup();
    const { onSelect, onContextMenu } = await setup([1]);
    await user.pointer({ keys: '[MouseRight]', target: row('sub') });
    expect(onSelect).toHaveBeenLastCalledWith([2]);
    expect(onContextMenu).toHaveBeenLastCalledWith([2], expect.any(Number), expect.any(Number));
    // right click inside the current selection keeps it
    await user.pointer({ keys: '[MouseRight]', target: row('movie.mkv') });
    expect(onContextMenu).toHaveBeenLastCalledWith([1], expect.any(Number), expect.any(Number));
  });

  test('column headers change the sort order', async () => {
    const user = userEvent.setup();
    const { model } = await setup();
    const spy = vi.spyOn(model, 'setSort').mockResolvedValue();
    await user.click(screen.getByRole('button', { name: /^Name/ }));
    expect(spy).toHaveBeenLastCalledWith('name', false);
    await user.click(screen.getByRole('button', { name: /^Size/ }));
    expect(spy).toHaveBeenLastCalledWith('size', false);
  });
});
