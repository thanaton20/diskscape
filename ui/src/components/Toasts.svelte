<script lang="ts">
  import { t } from '../lib/i18n';

  export interface Toast {
    id: number;
    /** Errors stay until dismissed and are announced at once. */
    error: boolean;
    title: string;
    lines: string[];
  }

  interface Props {
    toasts: Toast[];
    onDismiss: (id: number) => void;
  }
  let { toasts, onDismiss }: Props = $props();
</script>

<div class="toasts" aria-live="polite">
  {#each toasts as toast (toast.id)}
    <div class="toast" class:error={toast.error} role={toast.error ? 'alert' : undefined}>
      <div class="text">
        <div class="title">{toast.title}</div>
        {#if toast.lines.length}
          <ul>
            {#each toast.lines as line}<li>{line}</li>{/each}
          </ul>
        {/if}
      </div>
      <button aria-label={t('toast.close')} onclick={() => onDismiss(toast.id)}>×</button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 14px;
    bottom: 38px;
    z-index: 25;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: min(480px, calc(100% - 28px));
  }
  .toast {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 9px 10px 9px 13px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: 9px;
    box-shadow: var(--shadow);
    user-select: text;
  }
  .toast.error {
    border-left-color: var(--danger);
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .title {
    font-weight: 600;
  }
  ul {
    margin: 5px 0 0;
    padding-left: 16px;
    max-height: 160px;
    overflow: auto;
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  button {
    flex: none;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: none;
    font-size: 16px;
    line-height: 1;
    color: var(--muted);
    cursor: pointer;
  }
  button:hover {
    background: var(--panel-2);
    color: var(--text);
  }
</style>
