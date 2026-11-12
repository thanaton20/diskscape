<script lang="ts">
  import { onMount } from 'svelte';
  import { formatCount, formatDuration, formatSize } from '../lib/format';
  import { getLocale, t } from '../lib/i18n';
  import type { ProgressDto } from '../lib/types';

  interface Props {
    /** Latest progress event; `null` until the first one arrives. */
    progress: ProgressDto | null;
    /** `Date.now()` when the scan started. */
    startedAt: number;
    onCancel: () => void;
  }
  let { progress, startedAt, onCancel }: Props = $props();

  const locale = getLocale();
  let now = $state(Date.now());

  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 250);
    return () => clearInterval(timer);
  });
</script>

<main class="progress">
  <div class="card">
    <h1>{t('progress.heading')}</h1>
    <div class="sweep" aria-hidden="true"><div></div></div>
    <dl>
      <div>
        <dt>{t('progress.items')}</dt>
        <dd class="tabular" data-testid="items">{formatCount(progress?.items ?? 0, locale)}</dd>
      </div>
      <div>
        <dt>{t('progress.size')}</dt>
        <dd class="tabular">{formatSize(progress?.bytes ?? 0, locale)}</dd>
      </div>
      <div>
        <dt>{t('progress.elapsed')}</dt>
        <dd class="tabular" data-testid="elapsed">{formatDuration(now - startedAt, locale)}</dd>
      </div>
    </dl>
    <div class="current">
      <span class="label">{t('progress.current')}</span>
      <span class="path mono" title={progress?.current ?? ''}>{progress?.current ?? '…'}</span>
    </div>
    <button class="btn" onclick={onCancel}>{t('progress.cancel')}</button>
  </div>
</main>

<style>
  /* Flex, not a centred grid: an auto-sized grid track grows with the long
     unwrapped path below, and the card, centred in that track, slid sideways
     every time the current folder changed. */
  .progress {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 28px;
  }
  .card {
    flex: none;
    width: min(560px, 100%);
    min-width: 0;
    padding: 26px 28px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 650;
  }
  .sweep {
    height: 4px;
    border-radius: 2px;
    background: var(--bar-track);
    overflow: hidden;
  }
  .sweep div {
    width: 35%;
    height: 100%;
    border-radius: 2px;
    background: var(--accent);
    animation: sweep 1.3s ease-in-out infinite;
  }
  @keyframes sweep {
    from { transform: translateX(-100%); }
    to { transform: translateX(290%); }
  }
  @media (prefers-reduced-motion: reduce) {
    .sweep div { animation: none; width: 100%; opacity: 0.5; }
  }
  dl {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
    margin: 0;
  }
  dt,
  .label {
    font-size: 11px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--muted);
  }
  dd {
    margin: 2px 0 0;
    font-size: 20px;
    font-weight: 600;
  }
  .current {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
    color: var(--muted);
  }
  .btn {
    align-self: flex-start;
  }
</style>
