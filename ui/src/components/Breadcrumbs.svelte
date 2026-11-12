<script lang="ts">
  import * as api from '../lib/api';
  import { t } from '../lib/i18n';
  import type { NodeDto } from '../lib/types';

  interface Props {
    /** Folder currently shown on the map. */
    dirId: number;
    onNavigate: (id: number) => void;
  }
  let { dirId, onNavigate }: Props = $props();

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
