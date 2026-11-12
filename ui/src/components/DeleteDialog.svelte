<script lang="ts">
  import { onMount } from 'svelte';
  import { formatSize } from '../lib/format';
  import { getLocale, t } from '../lib/i18n';
  import type { SelectionInfo } from '../lib/types';

  interface Props {
    /** What is about to be deleted for good. */
    info: SelectionInfo;
    onConfirm: () => void;
    onCancel: () => void;
  }
  let { info, onConfirm, onCancel }: Props = $props();

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
