<script lang="ts">
  import Compass from '@lucide/svelte/icons/compass';
  import EmptyState from '../components/EmptyState.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import { href, router } from '../lib/router.svelte';

  let { params: _params }: { params: Record<string, string> } = $props();
</script>

<PageHeader title="Not found" />
<EmptyState icon={Compass} title="There's nothing at {router.path}">
  The page may have moved, or the container or stack was removed.
  {#snippet actions()}
    <a class="btn" href={href('/')}>Go to the dashboard</a>
  {/snippet}
</EmptyState>
