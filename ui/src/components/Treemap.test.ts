import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import Treemap from './Treemap.svelte';
import * as api from '../lib/api';
import { setLocale } from '../lib/i18n';
import { mockLayout, node } from '../lib/testing';
import type { RectDto } from '../lib/types';

vi.mock('../lib/api');

const rect = (id: number | null, x: number, y: number, w: number, h: number, isDir = false, depth = 0): RectDto =>
  ({ id, x, y, w, h, depth, isDir, category: 'video' });
// dir 2 on the left with file 5 inside; file 3 top right; "small items" bottom right
const rects = [
  rect(2, 0, 0, 400, 600, true),
  rect(3, 400, 0, 400, 300),
  rect(null, 400, 300, 400, 300),
  rect(5, 10, 10, 200, 200, false, 1),
];

let layout: ReturnType<typeof mockLayout>;

function mount(props: Partial<{ dirId: number; selected: number[]; version: number }> = {}) {
  const handlers = { onSelect: vi.fn(), onEnter: vi.fn() };
  const view = render(Treemap, { props: { dirId: 0, selected: [], version: 0, ...props, ...handlers } });
  return { ...handlers, ...view };
}

const canvas = () => screen.getByRole('img', { name: 'Map of disk usage' });

beforeEach(() => {
  vi.resetAllMocks();
  setLocale('en');
  layout = mockLayout(800, 600);
  vi.mocked(api.getTreemap).mockResolvedValue(rects);
  vi.mocked(api.getPath).mockImplementation(async (id) => `/r/item-${id}`);
  vi.mocked(api.getNodes).mockImplementation(async (ids) => ids.map((id) => node(id, `item-${id}`, 1610612736)));
});

afterEach(() => {
  layout.restore();
  vi.useRealTimers();
});

describe('Treemap', () => {
  test('requests the layout with its pixel size', async () => {
    mount({ dirId: 7 });
    await waitFor(() => expect(api.getTreemap).toHaveBeenCalledWith(7, 800, 600));
  });

  test('requests it again after a resize, debounced by 100 ms', async () => {
    vi.useFakeTimers();
    mount();
    await vi.advanceTimersByTimeAsync(0);
    expect(api.getTreemap).toHaveBeenCalledTimes(1);
    layout.resize(1000, 700);
    layout.resize(1010, 705);
    await vi.advanceTimersByTimeAsync(99);
    expect(api.getTreemap).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(api.getTreemap).toHaveBeenCalledTimes(2);
    expect(api.getTreemap).toHaveBeenLastCalledWith(0, 1010, 705);
  });

  test('asks again when the folder or the version changes', async () => {
    const { rerender } = mount({ dirId: 0, version: 0 });
    await waitFor(() => expect(api.getTreemap).toHaveBeenCalledTimes(1));
    await rerender({ dirId: 4 });
    await waitFor(() => expect(api.getTreemap).toHaveBeenLastCalledWith(4, 800, 600));
    await rerender({ version: 1 });
    await waitFor(() => expect(api.getTreemap).toHaveBeenCalledTimes(3));
  });

  test('does not ask for a layout while it has no size', async () => {
    layout.restore();
    layout = mockLayout(0, 0);
    mount();
    await new Promise((r) => setTimeout(r, 20));
    expect(api.getTreemap).not.toHaveBeenCalled();
  });

  test('click on a rect selects the deepest one; the aggregate block selects nothing', async () => {
    const { onSelect } = mount();
    await waitFor(() => expect(api.getTreemap).toHaveBeenCalled());
    await fireEvent.click(canvas(), { clientX: 50, clientY: 50 });
    expect(onSelect).toHaveBeenLastCalledWith(5);
    await fireEvent.click(canvas(), { clientX: 300, clientY: 400 });
    expect(onSelect).toHaveBeenLastCalledWith(2);
    onSelect.mockClear();
    await fireEvent.click(canvas(), { clientX: 600, clientY: 450 });
    expect(onSelect).not.toHaveBeenCalled();
  });

  test('double click on a directory enters it; on a file it does not', async () => {
    const { onEnter } = mount();
    await waitFor(() => expect(api.getTreemap).toHaveBeenCalled());
    await fireEvent.dblClick(canvas(), { clientX: 300, clientY: 400 });
    expect(onEnter).toHaveBeenCalledWith(2);
    await fireEvent.dblClick(canvas(), { clientX: 600, clientY: 100 });
    await fireEvent.dblClick(canvas(), { clientX: 600, clientY: 450 });
    expect(onEnter).toHaveBeenCalledTimes(1);
  });

  test('hover shows a tooltip with the path and size', async () => {
    mount();
    await waitFor(() => expect(api.getTreemap).toHaveBeenCalled());
    await fireEvent.mouseMove(canvas(), { clientX: 600, clientY: 100 });
    const tip = await screen.findByRole('tooltip');
    await waitFor(() => expect(tip).toHaveTextContent('/r/item-3'));
    expect(tip).toHaveTextContent('1.5 GB');
    await fireEvent.mouseMove(canvas(), { clientX: 600, clientY: 450 });
    await waitFor(() => expect(screen.getByRole('tooltip')).toHaveTextContent('Small items'));
    await fireEvent.mouseLeave(canvas());
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  test('selected nodes are outlined on the map', async () => {
    mount({ selected: [3, 5, 999] });
    await waitFor(() => expect(screen.getAllByTestId('map-selection')).toHaveLength(2));
    const boxes = screen.getAllByTestId('map-selection');
    expect(boxes[0].style.left).toBe('400px');
    expect(boxes[0].style.width).toBe('400px');
    expect(boxes[1].style.height).toBe('200px');
  });

  test('says so when there is nothing to show', async () => {
    vi.mocked(api.getTreemap).mockResolvedValue([]);
    mount();
    expect(await screen.findByText('Nothing to show here')).toBeInTheDocument();
  });
});
