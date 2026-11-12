<script lang="ts">
  import * as api from '../lib/api';
  import { t } from '../lib/i18n';
  import { platform } from '../lib/platform';

  interface Props {
    /** Folders the scan could not read. */
    count: number;
  }
  let { count }: Props = $props();
  let failure = $state<string | null>(null);

  async function ask() {
    failure = null;
    try {
      await api.requestElevation();
    } catch (e) {
      failure = String(e);
    }
  }
</script>

<div class="banner" role="status">
  <span class="text">
    {t('banner.unreadable', { n: count })}
    {#if platform === 'macos'}{t('banner.hint.macos')}{/if}
    {#if failure}<strong>{failure}</strong>{/if}
  </span>
  <button class="btn" onclick={ask}>{t(`banner.action.${platform}`)}</button>
</div>

<style>
  .banner {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 7px 12px;
    background: var(--warn-bg);
    color: var(--warn-text);
    border-bottom: 1px solid var(--border);
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .btn {
    flex: none;
    color: var(--text);
  }
</style>
