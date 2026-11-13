<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from '../lib/api';
  import Breadcrumbs from '../components/Breadcrumbs.svelte';
  import ContextMenu, { type MenuAction } from '../components/ContextMenu.svelte';
  import DeleteDialog from '../components/DeleteDialog.svelte';
  import Toasts, { type Toast } from '../components/Toasts.svelte';
  import StatusBar from '../components/StatusBar.svelte';
  import Treemap from '../components/Treemap.svelte';
  import TreeList from '../components/TreeList.svelte';
  import UnreadableBanner from '../components/UnreadableBanner.svelte';
  import { formatSize } from '../lib/format';
  import { getLocale, t } from '../lib/i18n';
  import { platform } from '../lib/platform';
  import { matchShortcut } from '../lib/shortcuts';
  import { TreeModel } from '../lib/treeModel.svelte';
  import type { DeleteMode, DeleteResult, NodeDto, SelectionInfo, Summary } from '../lib/types';

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
  // svelte-ignore state_referenced_locally
  const rootId = root.id;
  /** Folder shown on the map. */
  let mapDir = $state(rootId);
  /** Bumped whenever the tree changes, to make the map reload. */
  let mapVersion = $state(0);

  const INFO_TOAST_MS = 5000;
  const locale = getLocale();
  let menu = $state<{ x: number; y: number } | null>(null);
  const DIALOG_PATHS = 5;
  /**
   * Set while the "delete permanently?" dialog is open. It carries the ids the
   * dialog describes: confirming deletes exactly those, whatever the selection
   * has become in the meantime.
   */
  let confirming = $state<{ ids: number[]; info: SelectionInfo; paths: string[] } | null>(null);
  /** True while a delete is running; further actions wait for it. */
  let busy = false;
  let toasts = $state<Toast[]>([]);
  let nextToast = 0;
  /** Deleting and revealing need a tree scanned on this machine, and its id. */
  const canModify = $derived(summary !== null && !summary.readOnly);

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

  function toast(error: boolean, title: string, lines: string[] = []) {
    const id = ++nextToast;
    toasts = [...toasts, { id, error, title, lines }];
    if (!error) setTimeout(() => dismiss(id), INFO_TOAST_MS);
  }

  function dismiss(id: number) {
    toasts = toasts.filter((item) => item.id !== id);
  }

  const failure = (e: unknown) => toast(true, t('error.generic', { message: String(e) }));

  /** Every action on the selection starts here, and only from something the user did. */
  async function act(action: MenuAction) {
    menu = null;
    // What is selected right now is what the action is about; the selection
    // may change while the backend answers.
    const ids = [...selected];
    if (ids.length === 0 || busy) return;
    try {
      if (action === 'copyPath') {
        const paths = await Promise.all(ids.map((id) => api.getPath(id)));
        await api.copyText(paths.join('\n'));
        toast(false, t('toast.copied'));
      } else if (!canModify) {
        return;
      } else if (action === 'reveal') {
        if (ids.length === 1) await api.reveal(ids[0]);
      } else if (action === 'trash') {
        await remove('trash', ids);
      } else {
        // Permanent deletion always goes through the confirmation dialog.
        const [info, paths] = await Promise.all([
          api.selectionInfo(ids),
          Promise.all(ids.slice(0, DIALOG_PATHS).map((id) => api.getPath(id))),
        ]);
        confirming = { ids, info, paths };
      }
    } catch (e) {
      failure(e);
    }
  }

  async function remove(mode: DeleteMode, ids: number[]) {
    if (!summary || ids.length === 0 || busy) return;
    busy = true;
    try {
      await removeNow(mode, ids, summary.treeId);
    } finally {
      busy = false;
    }
  }

  async function removeNow(mode: DeleteMode, ids: number[], treeId: number) {
    // Paths are looked up first: a deleted node can no longer be asked for its path.
    const paths = new Map<number, string>();
    try {
      const found = await Promise.all(ids.map((id) => api.getPath(id)));
      found.forEach((path, i) => paths.set(ids[i], path));
    } catch {
      /* fall back to ids in messages */
    }

    let results: DeleteResult[];
    try {
      results = await api.deleteNodes(ids, mode, treeId);
    } catch (e) {
      failure(e);
      return;
    }

    const done = results.filter((r) => r.ok);
    const failed = results.filter((r) => !r.ok);
    if (done.length > 0) {
      const freed = done.reduce((sum, r) => sum + r.freed, 0);
      toast(false, t(mode === 'trash' ? 'toast.trashed' : 'toast.deleted', {
        count: done.length,
        size: formatSize(freed, locale),
      }));
    }
    if (failed.length > 0) {
      toast(
        true,
        t('toast.failed', { n: failed.length }),
        failed.map((r) => {
          const why = r.error ? t(`error.${r.error.kind}`, { message: r.error.message }) : '';
          return `${paths.get(r.id) ?? r.id} — ${why}`;
        }),
      );
    }
    await reload();
  }

  /** Brings the list, the map and the totals back in line with the tree. */
  async function reload() {
    try {
      await model.refresh();
      mapVersion += 1;
      summary = await api.getSummary();
      const [shown] = await api.getNodes([mapDir]);
      if (!shown) mapDir = rootId;
      await select([]);
    } catch (e) {
      failure(e);
    }
  }

  async function exportTree() {
    const path = await api.pickSavePath();
    if (!path) return;
    try {
      await api.exportNcdu(path);
      toast(false, t('toast.exported', { path }));
    } catch (e) {
      failure(e);
    }
  }

  function windowKey(event: KeyboardEvent) {
    // The menu and the dialog handle their own keys; shortcuts must not reach past them.
    if (menu || confirming) return;
    const target = event.target as HTMLElement | null;
    if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')) return;
    // Escape steps the map back out to the scanned folder.
    if (event.key === 'Escape' && mapDir !== rootId) {
      event.preventDefault();
      mapDir = rootId;
      return;
    }
    const action = matchShortcut(event, platform === 'macos');
    if (!action || selected.length === 0) return;
    event.preventDefault();
    void act(action);
  }

  /** A folder was double-clicked on the map: open it there and point at it in the list. */
  async function enterFromMap(id: number) {
    mapDir = id;
    await selectFromMap(id);
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

<svelte:window onkeydown={windowKey} />

<div class="result" data-testid="result-screen">
  <header class="toolbar">
    <button class="btn" onclick={onNewScan}>{t('result.newScan')}</button>
    <button class="btn" onclick={exportTree}>{t('result.export')}</button>
    <span class="path mono" title={summary?.rootPath ?? root.name}>{summary?.rootPath ?? root.name}</span>
    {#if summary?.readOnly}
      <span class="badge">{t('result.readOnly')}</span>
    {/if}
  </header>

  {#if summary && summary.unreadableDirs > 0 && !summary.readOnly}
    <UnreadableBanner
      count={summary.unreadableDirs}
      privacy={summary.unreadablePrivacy}
      fullAccess={summary.fullDiskAccess}
    />
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
      onOpen={(id) => { if (canModify) api.reveal(id).catch(failure); }}
      onEnter={(id) => (mapDir = id)}
      onContextMenu={(_ids, x, y) => (menu = { x, y })}
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
      <Breadcrumbs dirId={mapDir} {rootId} onNavigate={(id) => (mapDir = id)} />
      <div class="map-area">
        <Treemap
          dirId={mapDir}
          {selected}
          version={mapVersion}
          onSelect={selectFromMap}
          onEnter={enterFromMap}
        />
      </div>
    </div>
  </div>

  {#if menu}
    <ContextMenu
      x={menu.x}
      y={menu.y}
      {canModify}
      single={selected.length === 1}
      onAction={act}
      onClose={() => (menu = null)}
    />
  {/if}

  {#if confirming}
    <DeleteDialog
      info={confirming.info}
      paths={confirming.paths}
      more={confirming.ids.length - confirming.paths.length}
      onCancel={() => (confirming = null)}
      onConfirm={() => {
        const ids = confirming?.ids ?? [];
        confirming = null;
        void remove('permanent', ids);
      }}
    />
  {/if}

  <Toasts {toasts} onDismiss={dismiss} />

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
  /* The single row must be exactly as tall as the space left for it. With an
     automatic row, a long list made the row — and with it the map — as tall as
     all of the list's rows, far beyond the window: the map stretched until only
     an empty corner of it was in view, and the status bar slid off-screen. */
  .panes {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-rows: minmax(0, 1fr);
    overflow: hidden;
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
