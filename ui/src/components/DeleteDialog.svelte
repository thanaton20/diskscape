<script lang="ts">
  import { onMount } from 'svelte';
  import { formatSize } from '../lib/format';
  import { getLocale, t } from '../lib/i18n';
  import type { SelectionInfo } from '../lib/types';

  interface Props {
    /** What is about to be deleted for good. */
    info: SelectionInfo;
    /** The first few paths, so the dialog names what it is about. */
    paths: string[];
    /** How many more are selected beyond `paths`. */
    more: number;
    onConfirm: () => void;
    onCancel: () => void;
  }
  let { info, paths, more, onConfirm, onCancel }: Props = $props();

  const locale = getLocale();
  let cancelButton = $state<HTMLButtonElement>();
  let confirmButton = $state<HTMLButtonElement>();

  // Cancel has the focus, so pressing Enter out of habit cancels rather than deletes.
  onMount(() => cancelButton?.focus());

  function key(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      onCancel();
    } else if (event.key === 'Tab') {
      // Keep the focus inside the dialog.
      event.preventDefault();
      (document.activeElement === cancelButton ? confirmButton : cancelButton)?.focus();
    }
  }
</script>

<svelte:window onkeydown={key} />

<div class="backdrop" role="presentation">
  <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="delete-title" aria-describedby="delete-body">
    <h2 id="delete-title">{t('dialog.title')}</h2>
    <p id="delete-body">
      {t('dialog.body', { count: info.count, size: formatSize(info.dsize, locale) })}
    </p>
    {#if paths.length > 0}
      <ul class="paths mono">
        {#each paths as path}<li>{path}</li>{/each}
        {#if more > 0}<li class="more">{t('dialog.more', { n: more })}</li>{/if}
      </ul>
    {/if}
    {#if info.hasSystemPath}
      <p class="warning">{t('dialog.system')}</p>
    {/if}
    <div class="buttons">
      <button class="btn" bind:this={cancelButton} onclick={onCancel}>{t('dialog.cancel')}</button>
      <button class="btn danger" bind:this={confirmButton} onclick={onConfirm}>{t('dialog.confirm')}</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: grid;
    place-items: center;
    background: rgb(10 12 20 / 45%);
  }
  .dialog {
    width: min(440px, calc(100% - 40px));
    padding: 22px 24px 18px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: var(--shadow);
  }
  h2 {
    margin: 0 0 8px;
    font-size: 17px;
    font-weight: 650;
  }
  p {
    margin: 0 0 10px;
  }
  .paths {
    margin: 0 0 10px;
    padding: 8px 10px;
    list-style: none;
    max-height: 120px;
    overflow: auto;
    border-radius: 8px;
    background: var(--panel-2);
    overflow-wrap: anywhere;
    user-select: text;
  }
  .more {
    color: var(--muted);
  }
  .warning {
    padding: 9px 11px;
    border-radius: 8px;
    background: var(--warn-bg);
    color: var(--warn-text);
  }
  .buttons {
    margin-top: 16px;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
