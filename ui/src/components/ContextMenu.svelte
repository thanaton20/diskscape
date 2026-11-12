<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../lib/i18n';
  import { platform } from '../lib/platform';

  export type MenuAction = 'reveal' | 'copyPath' | 'trash' | 'permanent';

  interface Props {
    x: number;
    y: number;
    /** False for a tree opened from a file: nothing may be deleted or revealed. */
    canModify: boolean;
    /** True when exactly one object is selected. */
    single: boolean;
    onAction: (action: MenuAction) => void;
    onClose: () => void;
  }
  let { x, y, canModify, single, onAction, onClose }: Props = $props();

  let menu = $state<HTMLDivElement>();

  onMount(() => {
    menu?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  });

  function outside(event: Event) {
    if (menu && !menu.contains(event.target as Node)) onClose();
  }

  function key(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    const items = [...(menu?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = (at + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length;
    items[next]?.focus();
    event.preventDefault();
  }
</script>

<svelte:window onpointerdown={outside} onkeydown={key} onblur={onClose} />

<div
  class="menu"
  role="menu"
  bind:this={menu}
  style:left="{Math.max(4, Math.min(x, window.innerWidth - 220))}px"
  style:top="{Math.max(4, Math.min(y, window.innerHeight - 150))}px"
>
  <button role="menuitem" disabled={!canModify || !single} onclick={() => onAction('reveal')}>
    {t(`menu.reveal.${platform}`)}
  </button>
  <button role="menuitem" onclick={() => onAction('copyPath')}>{t('menu.copyPath')}</button>
  <hr />
  <button role="menuitem" disabled={!canModify} onclick={() => onAction('trash')}>
    {t('menu.trash')}
  </button>
  <button role="menuitem" class="danger" disabled={!canModify} onclick={() => onAction('permanent')}>
    {t('menu.permanent')}
  </button>
</div>

<style>
  .menu {
    position: fixed;
    z-index: 20;
    min-width: 200px;
    padding: 5px;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 9px;
    box-shadow: var(--shadow);
  }
  button {
    padding: 5px 10px;
    border: 0;
    border-radius: 5px;
    background: none;
    text-align: left;
    cursor: pointer;
  }
  button:hover:not(:disabled),
  button:focus-visible {
    background: var(--accent);
    color: var(--accent-contrast);
    outline: none;
  }
  button.danger:not(:disabled) {
    color: var(--danger);
  }
  button.danger:hover:not(:disabled),
  button.danger:focus-visible {
    background: var(--danger);
    color: var(--danger-contrast);
  }
  button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  hr {
    width: 100%;
    margin: 4px 0;
    border: 0;
    border-top: 1px solid var(--border);
  }
</style>
