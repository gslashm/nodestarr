<!-- Live logs of one container; resumes on its own when the container starts again. -->
<script lang="ts">
  import LogStream from '../../components/LogStream.svelte';
  import { apiUrl, qs, seg } from '../../lib/api';
  import { isUp, stateOf } from '../../lib/format';
  import type { ContainerDetail, ContainerView } from '../../lib/types';

  let { c }: { c: ContainerView; detail: ContainerDetail | null; reload: () => void } = $props();
</script>

<LogStream
  path="/containers/{seg(c.id)}/logs/stream"
  downloadUrl={apiUrl(`/containers/${seg(c.id)}/logs${qs({ format: 'text', tail: 'all', timestamps: 1 })}`)}
  running={isUp(stateOf(c))}
/>
