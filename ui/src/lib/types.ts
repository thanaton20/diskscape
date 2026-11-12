// Shapes exchanged with the Rust side. Mirrors src-tauri/src/dto.rs and actions.rs.

export type NodeKind = 'dir' | 'file' | 'symlink' | 'other';
export type Category =
  | 'video' | 'audio' | 'image' | 'archive' | 'document' | 'code' | 'app' | 'other';
export type SortKey = 'size' | 'name' | 'items';
export type DeleteMode = 'trash' | 'permanent';

export interface NodeDto {
  id: number;
  name: string;
  kind: NodeKind;
  /** Disk usage including descendants. */
  dsize: number;
  asize: number;
  /** Number of descendants. */
  items: number;
  hasChildren: boolean;
  category: Category;
  readError: boolean;
  childError: boolean;
  otherFs: boolean;
  hardlinkDup: boolean;
}

export interface RectDto {
  /** `null` for the block that stands for all items too small to draw. */
  id: number | null;
  x: number;
  y: number;
  w: number;
  h: number;
  depth: number;
  isDir: boolean;
  category: Category;
  /** Label to print on the block; empty when there is no room for one. */
  name: string;
}

export interface VolumeDto {
  name: string;
  mountPoint: string;
  total: number;
  free: number;
}

export interface ProgressDto {
  items: number;
  bytes: number;
  current: string;
}

export interface Summary {
  unreadableDirs: number;
  totalDsize: number;
  elapsedMs: number;
  /** Opened from a file: nothing may be deleted or revealed. */
  readOnly: boolean;
  rootPath: string;
  /** Identifies the tree; sent back with every delete. */
  treeId: number;
}

export interface SelectionInfo {
  count: number;
  dsize: number;
  hasSystemPath: boolean;
}

export type DeleteErrorKind =
  | 'readOnly' | 'notFound' | 'refused' | 'trashUnavailable' | 'partial' | 'io';

export interface DeleteResult {
  id: number;
  ok: boolean;
  freed: number;
  error: { kind: DeleteErrorKind; message: string } | null;
}
