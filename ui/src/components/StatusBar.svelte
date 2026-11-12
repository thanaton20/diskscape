<script lang="ts">
  import { formatDuration, formatSize } from '../lib/format';
  import { getLocale, t } from '../lib/i18n';

  interface Props {
    selectedCount: number;
    selectedSize: number;
    total: number;
    /** 0 when the tree was opened from a file. */
    elapsedMs: number;
  }
  let { selectedCount, selectedSize, total, elapsedMs }: Props = $props();
  const locale = getLocale();
</script>

<footer class="tabular">
  {#if selectedCount > 0}
    <span>{t('status.selected', { size: formatSize(selectedSize, locale) })}</span>
    {#if selectedCount > 1}
      <span class="muted">{t('status.selectedCount', { n: selectedCount })}</span>
    {/if}
  {/if}
  <span class="spring"></span>
  {#if elapsedMs > 0}
    <span class="muted">{t('status.time', { time: formatDuration(elapsedMs, locale) })}</span>
  {/if}
  <span>{t('status.total', { size: formatSize(total, locale) })}</span>
</footer>

<style>
  footer {
    flex: none;
    height: 26px;
    padding: 0 12px;
    display: flex;
    align-items: center;
    gap: 14px;
    border-top: 1px solid var(--border);
    background: var(--panel-2);
    font-size: 12px;
  }
  .spring {
    flex: 1;
  }
  .muted {
    color: var(--muted);
  }
</style>
