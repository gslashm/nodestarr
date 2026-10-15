<!--
  Nodestarr's mark: a star with a hub-and-spoke network inside it. The star
  topology is the idea behind the name: one hub (Nodestarr) connected to
  every node (your containers).
-->
<script lang="ts">
  let { size = 28 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
  <path
    d="M16 3.5 19.47 12.13 28.74 12.76 21.61 18.72 23.88 27.74 16 22.8 8.12 27.74 10.39 18.72 3.26 12.76 12.53 12.13Z"
    fill="var(--accent)"
    stroke="var(--accent)"
    stroke-width="1.8"
    stroke-linejoin="round"
  />
  <g fill="var(--accent-ink)" stroke="var(--accent-ink)" opacity="0.88">
    <path d="M16 16.9V9M16 16.9 23.51 14.46M16 16.9 20.64 23.29M16 16.9 11.36 23.29M16 16.9 8.49 14.46" fill="none" stroke-width="1.5" stroke-linecap="round" />
    <circle cx="16" cy="9" r="1.3" stroke="none" />
    <circle cx="23.51" cy="14.46" r="1.3" stroke="none" />
    <circle cx="20.64" cy="23.29" r="1.3" stroke="none" />
    <circle cx="11.36" cy="23.29" r="1.3" stroke="none" />
    <circle cx="8.49" cy="14.46" r="1.3" stroke="none" />
    <circle cx="16" cy="16.9" r="2" stroke="none" />
  </g>
</svg>
