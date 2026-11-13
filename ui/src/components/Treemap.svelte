<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from '../lib/api';
  import { formatSize } from '../lib/format';
  import { hitTest, hitTestFolder } from '../lib/hitTest';
  import { getLocale, t } from '../lib/i18n';
  import { categoryColor, folderColor, gapColor, type Theme } from '../lib/palette';
  import type { RectDto } from '../lib/types';

  interface Props {
    /** Folder whose contents are drawn. */
    dirId: number;
    selected: number[];
    /** Bumped by the parent whenever the tree changed and the map must be redrawn. */
    version: number;
    onSelect: (id: number) => void;
    /** A folder block was double-clicked. */
    onEnter: (id: number) => void;
    /** A block was right-clicked. */
    onContextMenu: (id: number, x: number, y: number) => void;
  }
  let { dirId, selected, version, onSelect, onEnter, onContextMenu }: Props = $props();

  const RESIZE_DEBOUNCE = 100;
  const locale = getLocale();

  let wrap = $state<HTMLDivElement>();
  let canvas = $state<HTMLCanvasElement>();
  let size = $state({ w: 0, h: 0 });
  let rects = $state.raw<RectDto[]>([]);
  let loaded = $state(false);
  let theme = $state<Theme>('light');
  let tip = $state<{ x: number; y: number; title: string; detail: string } | null>(null);
  let hoverId: number | null | undefined;

  const highlighted = $derived.by(() => {
    const ids = new Set(selected);
    return rects.filter((r) => r.id !== null && ids.has(r.id));
  });

  onMount(() => {
    if (!wrap) return;
    size = { w: wrap.clientWidth, h: wrap.clientHeight };
    let timer: ReturnType<typeof setTimeout> | undefined;
    const observer = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      if (!box) return;
      clearTimeout(timer);
      timer = setTimeout(() => {
        size = { w: Math.round(box.width), h: Math.round(box.height) };
      }, RESIZE_DEBOUNCE);
    });
    observer.observe(wrap);

    const dark = window.matchMedia?.('(prefers-color-scheme: dark)');
    const applyTheme = () => (theme = dark?.matches ? 'dark' : 'light');
    applyTheme();
    dark?.addEventListener?.('change', applyTheme);

    return () => {
      observer.disconnect();
      clearTimeout(timer);
      dark?.removeEventListener?.('change', applyTheme);
    };
  });

  // The layout is computed by the backend for the exact pixel size.
  $effect(() => {
    const { w, h } = size;
    const id = dirId;
    void version;
    if (w <= 0 || h <= 0) return;
    let stale = false;
    api
      .getTreemap(id, w, h)
      .then((result) => {
        if (stale) return;
        rects = result ?? [];
        loaded = true;
      })
      .catch(() => {
        if (!stale) rects = [];
      });
    return () => {
      stale = true;
    };
  });

  $effect(() => draw(rects, size, theme));

  function draw(list: RectDto[], { w, h }: { w: number; h: number }, mode: Theme) {
    const ctx = canvas?.getContext('2d');
    if (!canvas || !ctx || w <= 0 || h <= 0) return;
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.round(w * ratio);
    canvas.height = Math.round(h * ratio);
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.fillStyle = gapColor(mode);
    ctx.fillRect(0, 0, w, h);
    for (const r of list) {
      // Half-pixel inset leaves a hairline gap between neighbours.
      const [x, y, bw, bh] = [r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1];
      if (bw <= 0 || bh <= 0) continue;
      ctx.fillStyle = r.isDir ? folderColor(mode, r.depth) : categoryColor(r.category, mode, r.depth);
      ctx.fillRect(x, y, bw, bh);
      if (r.id === null) hatch(ctx, x, y, bw, bh, mode);
    }
    // Labels go on last so no later block paints over them.
    ctx.font = '11px system-ui, -apple-system, "Segoe UI", sans-serif';
    ctx.textBaseline = 'top';
    ctx.fillStyle = mode === 'light' ? 'rgb(18 22 38 / 88%)' : 'rgb(255 255 255 / 92%)';
    for (const r of list) {
      if (r.name) label(ctx, r.name, r.x + 5, r.y + 4, r.w - 10);
    }
  }

  /** Prints `text` in at most `width` pixels, cut with an ellipsis if it is longer. */
  function label(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, width: number) {
    let shown = text;
    while (shown.length > 1 && ctx.measureText(shown).width > width) {
      shown = `${shown.slice(0, shown.endsWith('…') ? -2 : -1)}…`;
    }
    if (ctx.measureText(shown).width <= width) ctx.fillText(shown, x, y);
  }

  /** Diagonal stripes mark the block that stands for many small items. */
  function hatch(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, mode: Theme) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(x, y, w, h);
    ctx.clip();
    ctx.strokeStyle = mode === 'light' ? 'rgb(255 255 255 / 55%)' : 'rgb(0 0 0 / 35%)';
    ctx.lineWidth = 2;
    ctx.beginPath();
    for (let offset = -h; offset < w; offset += 8) {
      ctx.moveTo(x + offset, y + h);
      ctx.lineTo(x + offset + h, y);
    }
    ctx.stroke();
    ctx.restore();
  }

  function at(event: MouseEvent) {
    const box = canvas?.getBoundingClientRect();
    const x = event.clientX - (box?.left ?? 0);
    const y = event.clientY - (box?.top ?? 0);
    return { x, y, hit: hitTest(rects, x, y) };
  }

  function click(event: MouseEvent) {
    const { hit } = at(event);
    if (hit && hit.id !== null) onSelect(hit.id);
  }

  function doubleClick(event: MouseEvent) {
    const { x, y } = at(event);
    const folder = hitTestFolder(rects, x, y);
    if (folder && folder.id !== null) onEnter(folder.id);
  }

  function contextMenu(event: MouseEvent) {
    // Never the webview's own menu, whatever is under the pointer.
    event.preventDefault();
    const { hit } = at(event);
    if (hit && hit.id !== null) onContextMenu(hit.id, event.clientX, event.clientY);
  }

  async function move(event: MouseEvent) {
    const { x, y, hit } = at(event);
    if (!hit) {
      leave();
      return;
    }
    if (hit.id === hoverId && tip) {
      tip = { ...tip, x, y };
      return;
    }
    hoverId = hit.id;
    if (hit.id === null) {
      tip = { x, y, title: t('map.small'), detail: '' };
      return;
    }
    tip = { x, y, title: '…', detail: '' };
    const id = hit.id;
    try {
      const [path, nodes] = await Promise.all([api.getPath(id), api.getNodes([id])]);
      // The pointer may have moved on while the answer was on its way.
      if (hoverId !== id || !tip) return;
      const node = nodes?.[0];
      tip = { ...tip, title: path, detail: node ? formatSize(node.dsize, locale) : '' };
    } catch {
      /* the tooltip is a nicety; leave the placeholder */
    }
  }

  function leave() {
    hoverId = undefined;
    tip = null;
  }
</script>

<div class="map" bind:this={wrap}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <canvas
    bind:this={canvas}
    role="img"
    aria-label={t('map.label')}
    onclick={click}
    ondblclick={doubleClick}
    oncontextmenu={contextMenu}
    onmousemove={move}
    onmouseleave={leave}
  ></canvas>

  {#each highlighted as r (r.id)}
    <div
      class="selection"
      data-testid="map-selection"
      style:left="{r.x}px"
      style:top="{r.y}px"
      style:width="{r.w}px"
      style:height="{r.h}px"
    ></div>
  {/each}

  {#if loaded && rects.length === 0}
    <p class="empty">{t('map.empty')}</p>
  {/if}

  {#if tip}
    <div
      class="tip"
      role="tooltip"
      style:left="{Math.min(tip.x + 14, Math.max(0, size.w - 260))}px"
      style:top="{Math.min(tip.y + 16, Math.max(0, size.h - 48))}px"
    >
      <span class="tip-title mono">{tip.title}</span>
      {#if tip.detail}<span class="tabular">{tip.detail}</span>{/if}
    </div>
  {/if}
</div>

<style>
  .map {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
  .selection {
    position: absolute;
    pointer-events: none;
    border: 2px solid var(--text);
    box-shadow: 0 0 0 1px var(--panel), inset 0 0 0 1px var(--panel);
  }
  .empty {
    position: absolute;
    inset: 0;
    margin: 0;
    display: grid;
    place-items: center;
    color: var(--muted);
  }
  .tip {
    position: absolute;
    z-index: 2;
    max-width: 260px;
    padding: 5px 8px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    pointer-events: none;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: var(--shadow);
  }
  .tip-title {
    overflow-wrap: anywhere;
  }
</style>
