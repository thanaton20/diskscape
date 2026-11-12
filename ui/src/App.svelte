<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from './lib/api';
  import type { NodeDto, ProgressDto } from './lib/types';
  import Progress from './screens/Progress.svelte';
  import Result from './screens/Result.svelte';
  import Start from './screens/Start.svelte';

  let screen = $state<'start' | 'progress' | 'result'>('start');
  let root = $state<NodeDto | null>(null);
  let error = $state<string | null>(null);
  let progress = $state<ProgressDto | null>(null);
  let startedAt = $state(0);

  onMount(() => {
    const subscriptions = [
      api.onScanProgress((p) => (progress = p)),
      api.onScanFinished((p) => {
        root = p.root;
        screen = 'result';
      }),
      api.onScanFailed((p) => {
        error = p.message;
        screen = 'start';
      }),
      // Cancelling discards the partial result and returns to the start screen.
      api.onScanCancelled(() => (screen = 'start')),
    ];
    return () => subscriptions.forEach((s) => s.then((off) => off?.()).catch(() => {}));
  });

  async function scan(path: string) {
    error = null;
    progress = null;
    startedAt = Date.now();
    screen = 'progress';
    try {
      await api.startScan(path);
    } catch (e) {
      error = String(e);
      screen = 'start';
    }
  }

  function showImported(imported: NodeDto) {
    error = null;
    root = imported;
    screen = 'result';
  }

  function newScan() {
    root = null;
    error = null;
    screen = 'start';
  }
</script>

{#if screen === 'progress'}
  <Progress {progress} {startedAt} onCancel={() => api.cancelScan()} />
{:else if screen === 'result' && root}
  {#key root.id + ':' + startedAt}
    <Result {root} onNewScan={newScan} />
  {/key}
{:else}
  <Start {error} onScan={scan} onImported={showImported} />
{/if}
