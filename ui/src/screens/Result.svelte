<script lang="ts">
  import type { NodeDto } from '../lib/types';

  interface Props {
    root: NodeDto;
    onNewScan: () => void;
  }
  let { root, onNewScan }: Props = $props();
</script>

<main data-testid="result-screen">
  <button class="btn" onclick={onNewScan}>←</button>
  {root.name}
</main>
