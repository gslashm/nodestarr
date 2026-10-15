<!-- Berth's mark: the door end of a shipping container. -->
<script lang="ts">
  let { size = 28 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
  <rect x="2.5" y="6.5" width="27" height="19" rx="2.6" fill="var(--accent)" />
  <path d="M8 10.2v11.6M12.2 10.2v11.6M19.8 10.2v11.6M24 10.2v11.6" stroke="var(--accent-ink)" stroke-opacity="0.42" stroke-width="1.7" stroke-linecap="round" />
  <path d="M16 9v14" stroke="var(--accent-ink)" stroke-opacity="0.75" stroke-width="1.7" stroke-linecap="round" />
  <path d="M14.2 14.5h1.1M16.7 17.5h1.1" stroke="var(--accent-ink)" stroke-opacity="0.75" stroke-width="1.7" stroke-linecap="round" />
</svg>
