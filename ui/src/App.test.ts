import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import App from './App.svelte';
import * as api from './lib/api';
import { setLocale } from './lib/i18n';
import type { NodeDto, ProgressDto } from './lib/types';

vi.mock('./lib/api');

const root: NodeDto = {
  id: 0, name: '/sample', kind: 'dir', dsize: 4096, asize: 5, items: 1, hasChildren: false,
  category: 'other', readError: false, childError: false, otherFs: false, hardlinkDup: false,
};

type Handlers = {
  progress?: (p: ProgressDto) => void;
  finished?: (p: { root: NodeDto }) => void;
  failed?: (p: { message: string }) => void;
  cancelled?: () => void;
};
let handlers: Handlers;

beforeEach(() => {
  vi.resetAllMocks();
  setLocale('en');
  handlers = {};
  const off = () => {};
  vi.mocked(api.onScanProgress).mockImplementation(async (cb) => ((handlers.progress = cb), off));
  vi.mocked(api.onScanFinished).mockImplementation(async (cb) => ((handlers.finished = cb), off));
  vi.mocked(api.onScanFailed).mockImplementation(async (cb) => ((handlers.failed = cb), off));
  vi.mocked(api.onScanCancelled).mockImplementation(async (cb) => ((handlers.cancelled = cb), off));
  vi.mocked(api.listVolumes).mockResolvedValue([
    { name: 'Macintosh HD', mountPoint: '/', total: 1000, free: 250 },
  ]);
  vi.mocked(api.initialPath).mockResolvedValue(null);
  vi.mocked(api.startScan).mockResolvedValue(undefined);
  vi.mocked(api.cancelScan).mockResolvedValue(undefined);
  vi.mocked(api.getChildren).mockResolvedValue([]);
  vi.mocked(api.getTreemap).mockResolvedValue([]);
  vi.mocked(api.getAncestors).mockResolvedValue([]);
  vi.mocked(api.getNodes).mockResolvedValue([root]);
  vi.mocked(api.selectionInfo).mockResolvedValue({ count: 0, dsize: 0, hasSystemPath: false });
  vi.mocked(api.getSummary).mockResolvedValue({
    unreadableDirs: 0, unreadablePrivacy: 0, fullDiskAccess: false, totalDsize: 4096, elapsedMs: 10, readOnly: false, rootPath: '/sample', treeId: 1,
  });
});

async function startScan() {
  render(App);
  await userEvent.click(await screen.findByRole('button', { name: /Macintosh HD/ }));
  await waitFor(() => expect(handlers.cancelled).toBeDefined());
  expect(api.startScan).toHaveBeenCalledWith('/');
  expect(screen.getByRole('button', { name: /cancel/i })).toBeInTheDocument();
}

describe('App', () => {
  test('starting a scan shows progress, and progress events update it', async () => {
    await startScan();
    handlers.progress!({ items: 42, bytes: 2048, current: '/usr' });
    expect(await screen.findByText('/usr')).toBeInTheDocument();
    expect(screen.getByTestId('items').textContent).toBe('42');
  });

  test('cancel asks the backend to stop; scan-cancelled returns to the start screen', async () => {
    await startScan();
    await userEvent.click(screen.getByRole('button', { name: /cancel/i }));
    expect(api.cancelScan).toHaveBeenCalledOnce();
    handlers.cancelled!();
    expect(await screen.findByRole('button', { name: /choose folder/i })).toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  test('scan-failed shows the message on the start screen', async () => {
    await startScan();
    handlers.failed!({ message: 'cannot read the scan root: permission denied' });
    expect(await screen.findByRole('alert')).toHaveTextContent('permission denied');
    expect(screen.getByRole('button', { name: /choose folder/i })).toBeInTheDocument();
  });

  test('scan-finished shows the result screen', async () => {
    await startScan();
    handlers.finished!({ root });
    expect(await screen.findByTestId('result-screen')).toBeInTheDocument();
  });

  test('a path given on the command line is scanned straight away', async () => {
    vi.mocked(api.initialPath).mockResolvedValue('/Users/me/Downloads');
    render(App);
    await waitFor(() => expect(api.startScan).toHaveBeenCalledWith('/Users/me/Downloads'));
    expect(await screen.findByRole('button', { name: /cancel/i })).toBeInTheDocument();
  });

  test('without one the start screen is shown and nothing is scanned', async () => {
    render(App);
    expect(await screen.findByRole('button', { name: /choose folder/i })).toBeInTheDocument();
    expect(api.startScan).not.toHaveBeenCalled();
  });

  test('a rejected start_scan returns to the start screen with the error', async () => {
    vi.mocked(api.startScan).mockRejectedValue('internal state is poisoned');
    render(App);
    await userEvent.click(await screen.findByRole('button', { name: /Macintosh HD/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent('poisoned');
  });
});
