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

/**
 * jsdom has no layout and no canvas. Gives every element a fixed size, makes
 * `getContext` quietly return nothing, and captures ResizeObserver callbacks
 * so a test can simulate a resize. Returns `{ resize, restore }`.
 */
export function mockLayout(width: number, height: number) {
  const size = { width, height };
  const observers: ResizeObserverCallback[] = [];
  const original = {
    clientWidth: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientWidth'),
    clientHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight'),
    getContext: HTMLCanvasElement.prototype.getContext,
    ResizeObserver: globalThis.ResizeObserver,
  };
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, get: () => size.width });
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => size.height });
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  globalThis.ResizeObserver = class {
    constructor(callback: ResizeObserverCallback) {
      observers.push(callback);
    }
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;

  return {
    resize(w: number, h: number) {
      size.width = w;
      size.height = h;
      const entry = { contentRect: { width: w, height: h } } as ResizeObserverEntry;
      observers.forEach((cb) => cb([entry], {} as ResizeObserver));
    },
    restore() {
      if (original.clientWidth) Object.defineProperty(HTMLElement.prototype, 'clientWidth', original.clientWidth);
      if (original.clientHeight) Object.defineProperty(HTMLElement.prototype, 'clientHeight', original.clientHeight);
      HTMLCanvasElement.prototype.getContext = original.getContext;
      globalThis.ResizeObserver = original.ResizeObserver;
    },
  };
}
