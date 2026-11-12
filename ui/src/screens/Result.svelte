<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from '../lib/api';
  import Breadcrumbs from '../components/Breadcrumbs.svelte';
  import StatusBar from '../components/StatusBar.svelte';
  import Treemap from '../components/Treemap.svelte';
  import TreeList from '../components/TreeList.svelte';
  import UnreadableBanner from '../components/UnreadableBanner.svelte';
  import { t } from '../lib/i18n';
  import { TreeModel } from '../lib/treeModel.svelte';
  import type { NodeDto, SelectionInfo, Summary } from '../lib/types';

  interface Props {
    root: NodeDto;
    onNewScan: () => void;
  }
  let { root, onNewScan }: Props = $props();

  const NOTHING: SelectionInfo = { count: 0, dsize: 0, hasSystemPath: false };
  const SPLIT_MIN = 20;
  const SPLIT_MAX = 80;

  // svelte-ignore state_referenced_locally
  const model = new TreeModel(root);
  let summary = $state<Summary | null>(null);
  let selected = $state<number[]>([]);
  let selection = $state<SelectionInfo>(NOTHING);
  /** Width of the list as a percentage of the window. */
  let split = $state(45);
  let panes = $state<HTMLDivElement>();
  let dragging = $state(false);
  let list = $state<TreeList>();
  /** Folder shown on the map. */
  // svelte-ignore state_referenced_locally
  let mapDir = $state(root.id);
  /** Bumped whenever the tree changes, to make the map reload. */
  let mapVersion = $state(0);

  onMount(async () => {
    await model.expand(model.rows[0].node.id);
    summary = await api.getSummary();
  });

  let selectionRequest = 0;
  async function select(ids: number[]) {
    selected = ids;
    const request = ++selectionRequest;
    const info = ids.length ? await api.selectionInfo(ids) : NOTHING;
    // Ignore an answer that a newer selection has overtaken.
    if (request === selectionRequest) selection = info;
  }

  /** A block was clicked on the map: open the list down to it and select it there. */
  async function selectFromMap(id: number) {
    const index = await model.revealNode(id);
    await select([id]);
    list?.scrollToIndex(index);
  }

  const clamp = (value: number) => Math.round(Math.max(SPLIT_MIN, Math.min(SPLIT_MAX, value)));

  function dividerKey(event: KeyboardEvent) {
    if (event.key === 'ArrowLeft') split = clamp(split - 2);
    else if (event.key === 'ArrowRight') split = clamp(split + 2);
    else return;
    event.preventDefault();
  }

  function dragTo(event: PointerEvent) {
    if (!dragging || !panes) return;
    const box = panes.getBoundingClientRect();
    if (box.width > 0) split = clamp(((event.clientX - box.left) / box.width) * 100);
  }
</script>

<div class="result" data-testid="result-screen">
  <header class="toolbar">
    <button class="btn" onclick={onNewScan}>{t('result.newScan')}</button>
    <span class="path mono" title={summary?.rootPath ?? root.name}>{summary?.rootPath ?? root.name}</span>
    {#if summary?.readOnly}
      <span class="badge">{t('result.readOnly')}</span>
    {/if}
  </header>

  {#if summary && summary.unreadableDirs > 0 && !summary.readOnly}
    <UnreadableBanner count={summary.unreadableDirs} />
  {/if}

  <div
    class="panes"
    class:dragging
    bind:this={panes}
    style:grid-template-columns="{split}% 5px minmax(0, 1fr)"
    onpointermove={dragTo}
    onpointerup={() => (dragging = false)}
    onpointerleave={() => (dragging = false)}
    role="presentation"
  >
    <TreeList
      bind:this={list}
      {model}
      {selected}
      onSelect={select}
      onOpen={() => {}}
      onContextMenu={() => {}}
    />
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="divider"
      role="separator"
      aria-orientation="vertical"
      aria-label={t('result.resize')}
      aria-valuemin={SPLIT_MIN}
      aria-valuemax={SPLIT_MAX}
      aria-valuenow={split}
      tabindex="0"
      onkeydown={dividerKey}
      onpointerdown={(e) => { dragging = true; e.preventDefault(); }}
    ></div>
    <div class="map-pane">
      <Breadcrumbs dirId={mapDir} onNavigate={(id) => (mapDir = id)} />
      <div class="map-area">
        <Treemap
          dirId={mapDir}
          {selected}
          version={mapVersion}
          onSelect={selectFromMap}
          onEnter={(id) => (mapDir = id)}
        />
      </div>
    </div>
  </div>

  <StatusBar
    selectedCount={selection.count}
    selectedSize={selection.dsize}
    total={summary?.totalDsize ?? model.rows[0]?.node.dsize ?? 0}
    elapsedMs={summary?.elapsedMs ?? 0}
  />
</div>

<style>
  .result {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .toolbar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
  }
  .badge {
    flex: none;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    font-size: 11px;
    color: var(--muted);
  }
  .panes {
    flex: 1;
    min-height: 0;
    display: grid;
  }
  .panes.dragging {
    cursor: col-resize;
  }
  .divider {
    background: var(--border);
    cursor: col-resize;
  }
  .divider:hover,
  .divider:focus-visible,
  .dragging .divider {
    background: var(--accent);
    outline: none;
  }
  .map-pane {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--panel-2);
  }
  .map-area {
    flex: 1;
    min-height: 0;
  }
</style>
