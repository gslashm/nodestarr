<!-- Relative time that stays current ("5 min ago"), with the full date on hover. -->
<script lang="ts">
  import { ago, dateTime } from '../lib/format';
  import { clock } from '../lib/clock.svelte';

  let { ts, prefix = '' }: { ts: number | null | undefined; prefix?: string } = $props();

  const text = $derived.by(() => {
    void clock.tick;
    return ago(ts);
  });
</script>

<time datetime={ts ? new Date(ts * 1000).toISOString() : undefined} title={dateTime(ts)}>{prefix}{text}</time>
