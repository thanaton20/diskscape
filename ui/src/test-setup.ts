import '@testing-library/jest-dom/vitest';

// jsdom has no layout engine, so it has no ResizeObserver either.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver;
