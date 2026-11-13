<script lang="ts">
  import * as api from '../lib/api';
  import { t } from '../lib/i18n';
  import type { NodeDto } from '../lib/types';

  interface Props {
    /** Folder currently shown on the map. */
    dirId: number;
    /** The scanned folder: where the back button leads. */
    rootId: number;
    onNavigate: (id: number) => void;
  }
  let { dirId, rootId, onNavigate }: Props = $props();

  let ancestors = $state.raw<NodeDto[]>([]);
  let current = $state.raw<NodeDto | null>(null);

  $effect(() => {
    const id = dirId;
    let stale = false;
    Promise.all([api.getAncestors(id), api.getNodes([id])])
      .then(([above, nodes]) => {
        if (stale) return;
        ancestors = above ?? [];
        current = nodes?.[0] ?? null;
      })
      .catch(() => {});
    return () => {
      stale = true;
    };
  });
</script>

<nav class="crumbs" aria-label={t('crumbs.label')}>
  {#if dirId !== rootId}
    <button class="back" aria-label={t('map.back')} title="{t('map.back')} (Esc)" onclick={() => onNavigate(rootId)}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M9.5 3 4.5 8l5 5" /></svg>
    </button>
  {/if}
  {#each ancestors as ancestor (ancestor.id)}
    <button onclick={() => onNavigate(ancestor.id)}>{ancestor.name}</button>
    <span class="sep" aria-hidden="true">›</span>
  {/each}
  {#if current}
    <span class="current">{current.name}</span>
  {/if}
</nav>

<style>
  .crumbs {
    flex: none;
    height: 28px;
    padding: 0 8px;
    display: flex;
    align-items: center;
    gap: 2px;
    overflow: hidden;
    white-space: nowrap;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }
  button {
    padding: 2px 5px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--accent);
    cursor: pointer;
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  button:hover {
    background: var(--row-hover);
  }
  button.back {
    flex: none;
    width: 22px;
    height: 20px;
    margin-right: 4px;
    padding: 0;
    display: grid;
    place-items: center;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--panel-2);
    color: var(--text);
  }
  button.back svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .sep {
    color: var(--muted);
  }
  .current {
    padding: 2px 5px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
