// Helpers shared by tests.
import type { NodeDto, NodeKind } from './types';

export function node(id: number, name: string, dsize: number, extra: Partial<NodeDto> = {}): NodeDto {
  const kind: NodeKind = extra.kind ?? 'file';
  return {
    id, name, kind, dsize, asize: dsize, items: 0, hasChildren: false, category: 'other',
    readError: false, childError: false, otherFs: false, hardlinkDup: false, ...extra,
  };
}

export const dir = (id: number, name: string, dsize: number, extra: Partial<NodeDto> = {}) =>
  node(id, name, dsize, { kind: 'dir', hasChildren: true, items: 1, ...extra });
