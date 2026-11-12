<script lang="ts">
  import * as api from '../lib/api';
  import { t } from '../lib/i18n';
  import { platform, type Platform } from '../lib/platform';

  interface Props {
    /** Folders the scan could not read. */
    count: number;
    /** How many of them the operating system's privacy protection blocked. */
    privacy: number;
    /** macOS: Full Disk Access is already granted to this app. */
    fullAccess: boolean;
    /** Overridable for tests. */
    os?: Platform;
  }
  let { count, privacy, fullAccess, os = platform }: Props = $props();
  let failure = $state<string | null>(null);

  // On macOS Full Disk Access is worth offering only if it is not granted yet
  // and something was actually blocked by privacy protection. Folders owned by
  // the system or by other users stay closed whatever is granted — and so do
  // some of macOS's own folders — so offering the setting for those would be
  // advice that cannot work.
  const offerAccess = $derived(!fullAccess && privacy > 0);
  const actionable = $derived(os !== 'macos' || offerAccess);
  const macNote = $derived(
    fullAccess ? t('banner.granted') : offerAccess ? t('banner.privacy', { n: privacy }) : t('banner.system'),
  );

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
    {#if os === 'macos'}{macNote}{/if}
    {#if failure}<strong>{failure}</strong>{/if}
  </span>
  {#if actionable}
    <button class="btn" onclick={ask}>{t(`banner.action.${os}`)}</button>
  {/if}
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
