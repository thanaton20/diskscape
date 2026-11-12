// Thin wrappers over the Tauri commands and events; the only file that talks to the backend.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import type {
  DeleteMode, DeleteResult, NodeDto, ProgressDto, RectDto, SelectionInfo, SortKey, Summary,
  VolumeDto,
} from './types';

/** Path given on the command line (`diskscape <path>`), if any. */
export const initialPath = () => invoke<string | null>('initial_path');
export const listVolumes = () => invoke<VolumeDto[]>('list_volumes');
export const startScan = (path: string) => invoke<void>('start_scan', { path });
export const cancelScan = () => invoke<void>('cancel_scan');
export const getChildren = (id: number, sort: SortKey, desc: boolean) =>
  invoke<NodeDto[]>('get_children', { id, sort, desc });
export const getNodes = (ids: number[]) => invoke<(NodeDto | null)[]>('get_nodes', { ids });
export const getAncestors = (id: number) => invoke<NodeDto[]>('get_ancestors', { id });
export const getPath = (id: number) => invoke<string>('get_path', { id });
export const getTreemap = (id: number, width: number, height: number) =>
  invoke<RectDto[]>('get_treemap', { id, width, height });
export const getSummary = () => invoke<Summary>('get_summary');
export const importNcdu = (path: string) => invoke<NodeDto>('import_ncdu', { path });
export const exportNcdu = (path: string) => invoke<void>('export_ncdu', { path });
export const selectionInfo = (ids: number[]) =>
  invoke<SelectionInfo>('get_selection_info', { ids });
/** `treeId` is `Summary.treeId`; the backend refuses if the tree has been replaced since. */
export const deleteNodes = (ids: number[], mode: DeleteMode, treeId: number) =>
  invoke<DeleteResult[]>('delete_nodes', { ids, mode, treeId });
export const reveal = (id: number) => invoke<void>('reveal', { id });
export const requestElevation = () => invoke<void>('request_elevation');

const single = (picked: string | string[] | null): string | null =>
  Array.isArray(picked) ? (picked[0] ?? null) : picked;

export const pickFolder = async () => single(await open({ directory: true, multiple: false }));
export const pickNcduFile = async () =>
  single(await open({ multiple: false, filters: [{ name: 'ncdu export', extensions: ['json'] }] }));
export const pickSavePath = () =>
  save({ defaultPath: 'diskscape-export.json', filters: [{ name: 'ncdu export', extensions: ['json'] }] });

export const onScanProgress = (cb: (p: ProgressDto) => void): Promise<UnlistenFn> =>
  listen<ProgressDto>('scan-progress', (e) => cb(e.payload));
export const onScanFinished = (cb: (p: { root: NodeDto }) => void): Promise<UnlistenFn> =>
  listen<{ root: NodeDto }>('scan-finished', (e) => cb(e.payload));
export const onScanFailed = (cb: (p: { message: string }) => void): Promise<UnlistenFn> =>
  listen<{ message: string }>('scan-failed', (e) => cb(e.payload));
export const onScanCancelled = (cb: () => void): Promise<UnlistenFn> =>
  listen('scan-cancelled', () => cb());
