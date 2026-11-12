<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from '../lib/api';
  import { formatSize } from '../lib/format';
  import { getLocale, t } from '../lib/i18n';
  import type { NodeDto, VolumeDto } from '../lib/types';

  interface Props {
    /** Why the last scan failed, if it did. */
    error: string | null;
    onScan: (path: string) => void;
    onImported: (root: NodeDto) => void;
  }
  let { error, onScan, onImported }: Props = $props();

  let volumes = $state<VolumeDto[]>([]);
  let loaded = $state(false);
  let importError = $state<string | null>(null);
  const locale = getLocale();

  onMount(async () => {
    try {
      volumes = await api.listVolumes();
    } catch {
      volumes = [];
    }
    loaded = true;
  });

  const usedPercent = (v: VolumeDto) =>
    v.total > 0 ? Math.round(((v.total - v.free) / v.total) * 100) : 0;

  async function chooseFolder() {
    const path = await api.pickFolder();
    if (path) onScan(path);
  }

  async function openNcdu() {
    importError = null;
    const path = await api.pickNcduFile();
    if (!path) return;
    try {
      onImported(await api.importNcdu(path));
    } catch (e) {
      importError = String(e);
    }
  }
</script>

<main class="start">
  <header>
    <h1>{t('start.heading')}</h1>
    <p>{t('start.subtitle')}</p>
  </header>

  {#if error || importError}
    <div class="alert" role="alert">
      {error ? t('start.error', { message: error }) : importError}
    </div>
  {/if}

  <section aria-labelledby="volumes-title">
    <h2 id="volumes-title">{t('start.volumes')}</h2>
    {#if loaded && volumes.length === 0}
      <p class="muted">{t('start.noVolumes')}</p>
    {/if}
    <div class="volumes">
      {#each volumes as volume (volume.mountPoint)}
        <button class="volume" onclick={() => onScan(volume.mountPoint)}>
          <span class="volume-head">
            <span class="volume-name">{volume.name || volume.mountPoint}</span>
            <span class="volume-path mono">{volume.mountPoint}</span>
          </span>
          <div
            class="meter"
            role="progressbar"
            aria-label={t('start.usage')}
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={usedPercent(volume)}
          >
            <div class="meter-fill" class:full={usedPercent(volume) >= 90} style:width="{usedPercent(volume)}%"></div>
          </div>
          <span class="volume-foot tabular">
            <span>
              {t('start.used', {
                used: formatSize(volume.total - volume.free, locale),
                total: formatSize(volume.total, locale),
              })}
            </span>
            <span>{t('start.free', { free: formatSize(volume.free, locale) })}</span>
          </span>
        </button>
      {/each}
    </div>
  </section>

  <div class="actions">
    <button class="btn primary" onclick={chooseFolder}>{t('start.chooseFolder')}</button>
    <button class="btn" onclick={openNcdu}>{t('start.openNcdu')}</button>
  </div>
</main>

<style>
  .start {
    height: 100%;
    overflow: auto;
    max-width: 720px;
    margin: 0 auto;
    padding: 56px 28px 40px;
    display: flex;
    flex-direction: column;
    gap: 22px;
  }
  h1 {
    margin: 0 0 6px;
    font-size: 26px;
    font-weight: 650;
    letter-spacing: -0.02em;
  }
  header p,
  .muted {
    margin: 0;
    color: var(--muted);
  }
  h2 {
    margin: 0 0 10px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .volumes {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .volume {
    display: flex;
    flex-direction: column;
    gap: 9px;
    width: 100%;
    padding: 14px 16px;
    text-align: left;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    cursor: pointer;
    transition: border-color 0.12s, transform 0.12s;
  }
  .volume:hover {
    border-color: var(--accent);
    transform: translateY(-1px);
  }
  .volume-head,
  .volume-foot {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
  }
  .volume-name {
    font-size: 15px;
    font-weight: 600;
  }
  .volume-path,
  .volume-foot {
    color: var(--muted);
  }
  .meter {
    height: 8px;
    border-radius: 4px;
    background: var(--bar-track);
    overflow: hidden;
  }
  .meter-fill {
    height: 100%;
    border-radius: 4px;
    background: var(--accent);
  }
  .meter-fill.full {
    background: var(--danger);
  }
  .actions {
    display: flex;
    gap: 10px;
  }
</style>
