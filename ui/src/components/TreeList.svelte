<script lang="ts">
  import { formatCount, formatSize } from '../lib/format';
  import { getLocale, t } from '../lib/i18n';
  import type { Row, TreeModel } from '../lib/treeModel.svelte';
  import type { SortKey } from '../lib/types';

  interface Props {
    model: TreeModel;
    selected: number[];
    onSelect: (ids: number[]) => void;
    /** A file was double-clicked or Enter was pressed on it. */
    onOpen: (id: number) => void;
    onContextMenu: (ids: number[], x: number, y: number) => void;
  }
  let { model, selected, onSelect, onOpen, onContextMenu }: Props = $props();

  const ROW = 24;
  const OVERSCAN = 10;
  const locale = getLocale();

  let viewport = $state<HTMLDivElement>();
  let height = $state(0);
  let scrollTop = $state(0);
  /** Row a shift-click range starts from. */
  let anchor = $state<number | null>(null);

  // Only the rows in view (plus a margin) are in the DOM, however long the list is.
  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(
    Math.min(model.rows.length, Math.ceil((scrollTop + (height || 600)) / ROW) + OVERSCAN),
  );
  const visible = $derived(model.rows.slice(first, last));
  const selectedSet = $derived(new Set(selected));

  export function scrollToIndex(index: number) {
    if (!viewport || index < 0) return;
    const top = index * ROW;
    const view = height || 600;
    if (top < viewport.scrollTop || top + ROW > viewport.scrollTop + view) {
      viewport.scrollTop = Math.max(0, top - view / 2);
    }
  }

  function toggle(row: Row) {
    if (row.expanded) collapse(row);
    else void model.expand(row.node.id);
  }

  /** Collapses a folder and deselects anything it hides: what cannot be seen must not be acted on. */
  function collapse(row: Row) {
    model.collapse(row.node.id);
    const stillShown = selected.filter((id) => model.indexOf(id) >= 0);
    if (stillShown.length !== selected.length) onSelect(stillShown);
  }

  function click(event: MouseEvent, row: Row, index: number) {
    const id = row.node.id;
    if (event.shiftKey && anchor !== null) {
      const from = model.indexOf(anchor);
      if (from >= 0) {
        const [lo, hi] = from < index ? [from, index] : [index, from];
        onSelect(model.rows.slice(lo, hi + 1).map((r) => r.node.id));
        return;
      }
    }
    anchor = id;
    if (event.ctrlKey || event.metaKey) {
      onSelect(selectedSet.has(id) ? selected.filter((s) => s !== id) : [...selected, id]);
    } else {
      onSelect([id]);
    }
  }

  function doubleClick(row: Row) {
    if (row.node.hasChildren) toggle(row);
    else if (row.node.kind !== 'dir') onOpen(row.node.id);
  }

  function contextMenu(event: MouseEvent, row: Row) {
    event.preventDefault();
    const id = row.node.id;
    let ids = selected;
    if (!selectedSet.has(id)) {
      ids = [id];
      anchor = id;
      onSelect(ids);
    }
    onContextMenu(ids, event.clientX, event.clientY);
  }

  function keydown(event: KeyboardEvent) {
    const current = selected.length ? model.indexOf(selected[selected.length - 1]) : -1;
    const row = model.rows[current];
    const move = (index: number) => {
      const target = model.rows[Math.max(0, Math.min(model.rows.length - 1, index))];
      if (!target) return;
      anchor = target.node.id;
      onSelect([target.node.id]);
      scrollToIndex(model.indexOf(target.node.id));
    };
    switch (event.key) {
      case 'ArrowDown': move(current + 1); break;
      case 'ArrowUp': move(current - 1); break;
      case 'ArrowRight': if (row && !row.expanded) void model.expand(row.node.id); break;
      case 'ArrowLeft': if (row?.expanded) collapse(row); break;
      case 'Enter': if (row) doubleClick(row); break;
      default: return;
    }
    event.preventDefault();
  }

  function sortBy(key: SortKey) {
    // Same column flips the direction; a new column starts with its natural order.
    void model.setSort(key, model.sort === key ? !model.desc : key !== 'name');
  }

  const arrow = (key: SortKey) => (model.sort === key ? (model.desc ? '↓' : '↑') : '');
</script>

<div class="list">
  <div class="head">
    <button class="col-name" onclick={() => sortBy('name')}>
      {t('col.name')} <span aria-hidden="true">{arrow('name')}</span>
    </button>
    <button class="col-size" onclick={() => sortBy('size')}>
      {t('col.size')} <span aria-hidden="true">{arrow('size')}</span>
    </button>
    <span class="col-share">{t('col.share')}</span>
    <button class="col-items" onclick={() => sortBy('items')}>
      {t('col.items')} <span aria-hidden="true">{arrow('items')}</span>
    </button>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="viewport"
    role="treegrid"
    aria-rowcount={model.rows.length}
    aria-multiselectable="true"
    tabindex="0"
    bind:this={viewport}
    bind:clientHeight={height}
    onscroll={() => (scrollTop = viewport?.scrollTop ?? 0)}
    onkeydown={keydown}
  >
    <div class="spacer" style:height="{model.rows.length * ROW}px">
      {#each visible as row, i (row.node.id)}
        {@const node = row.node}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="row"
          class:selected={selectedSet.has(node.id)}
          class:dim={node.hardlinkDup || node.otherFs}
          role="row"
          tabindex="-1"
          aria-level={row.depth + 1}
          aria-rowindex={first + i + 1}
          aria-selected={selectedSet.has(node.id)}
          aria-expanded={node.hasChildren ? row.expanded : undefined}
          style:transform="translateY({(first + i) * ROW}px)"
          onclick={(e) => click(e, row, first + i)}
          ondblclick={() => doubleClick(row)}
          oncontextmenu={(e) => contextMenu(e, row)}
        >
          <span class="col-name" role="gridcell" style:padding-left="{8 + row.depth * 16}px">
            {#if node.hasChildren}
              <button
                class="twisty"
                class:open={row.expanded}
                aria-label={row.expanded ? t('row.collapse') : t('row.expand')}
                onclick={(e) => { e.stopPropagation(); toggle(row); }}
                ondblclick={(e) => e.stopPropagation()}
              >
                <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M3 1.5 7 5 3 8.5z" /></svg>
              </button>
            {:else}
              <span class="twisty"></span>
            {/if}
            <span class="icon {node.kind}" aria-hidden="true"></span>
            <span class="label">{node.name}</span>
            {#if node.kind === 'symlink'}
              <span class="flag" title={t('row.symlink')}>↗</span>
            {/if}
            {#if node.hardlinkDup}
              <span class="flag" title={t('row.hardlink')}>⛓</span>
            {/if}
            {#if node.otherFs}
              <span class="flag" title={t('row.otherFs')}>⏏</span>
            {/if}
            {#if node.readError}
              <span class="flag warn" title={t('row.readError')}>⚠</span>
            {:else if node.childError}
              <span class="flag warn" title={t('row.childError')}>⚠</span>
            {/if}
          </span>
          <span class="col-size tabular" role="gridcell">{formatSize(node.dsize, locale)}</span>
          <span class="col-share" role="gridcell">
            <span
              class="meter"
              role="meter"
              aria-label={t('col.share')}
              aria-valuemin="0"
              aria-valuemax="100"
              aria-valuenow={Math.round(row.share * 100)}
            >
              <span style:width="{row.share * 100}%"></span>
            </span>
          </span>
          <span class="col-items tabular" role="gridcell">
            {node.kind === 'dir' && !node.otherFs ? formatCount(node.items, locale) : ''}
          </span>
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .list {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--panel);
  }
  .head,
  .row {
    display: grid;
    grid-template-columns: minmax(120px, 1fr) 84px 90px 78px;
    align-items: center;
  }
  .head {
    flex: none;
    height: 28px;
    border-bottom: 1px solid var(--border);
    color: var(--muted);
    font-size: 11px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .head button,
  .head span.col-share {
    height: 100%;
    padding: 0 8px;
    border: 0;
    background: none;
    text-align: left;
    text-transform: inherit;
    letter-spacing: inherit;
    font-size: inherit;
    color: inherit;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .head button {
    cursor: pointer;
  }
  .head button:hover {
    color: var(--text);
  }
  .head .col-size,
  .head .col-items {
    justify-content: flex-end;
  }
  .viewport {
    flex: 1;
    min-height: 0;
    overflow: auto;
    outline: none;
  }
  .spacer {
    position: relative;
    min-width: 100%;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 24px;
  }
  .row:hover {
    background: var(--row-hover);
  }
  .row.selected {
    background: var(--row-selected);
  }
  .row.dim .label,
  .row.dim .col-size {
    color: var(--muted);
  }
  .col-name {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    padding-right: 8px;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .col-size,
  .col-items {
    padding: 0 8px;
    text-align: right;
    white-space: nowrap;
  }
  .col-items {
    color: var(--muted);
  }
  .col-share {
    padding: 0 8px;
  }
  .meter {
    display: block;
    height: 6px;
    border-radius: 3px;
    background: var(--bar-track);
    overflow: hidden;
  }
  .meter span {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: var(--bar);
  }
  .twisty {
    flex: none;
    width: 14px;
    height: 14px;
    padding: 0;
    border: 0;
    background: none;
    display: grid;
    place-items: center;
    cursor: pointer;
    color: var(--muted);
  }
  .twisty svg {
    width: 9px;
    height: 9px;
    fill: currentColor;
    transition: transform 0.1s;
  }
  .twisty.open svg {
    transform: rotate(90deg);
  }
  .icon {
    flex: none;
    width: 14px;
    height: 12px;
    border-radius: 2px;
    background: var(--muted);
    opacity: 0.55;
  }
  .icon.dir {
    background: var(--accent);
    opacity: 0.85;
    border-radius: 2px 3px 2px 2px;
    box-shadow: inset 0 3px 0 rgb(255 255 255 / 25%);
  }
  .icon.file {
    width: 11px;
    height: 14px;
    margin: 0 1.5px;
  }
  .icon.symlink,
  .icon.other {
    width: 11px;
    height: 14px;
    margin: 0 1.5px;
    opacity: 0.3;
  }
  .flag {
    flex: none;
    font-size: 11px;
    color: var(--muted);
  }
  .flag.warn {
    color: #d98200;
  }
</style>
