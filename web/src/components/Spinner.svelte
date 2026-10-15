<script lang="ts">
  let { size = 16, label = 'Working' }: { size?: number; label?: string } = $props();
</script>

<svg class="spinner" width={size} height={size} viewBox="0 0 24 24" role="status" aria-label={label}>
  <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-opacity="0.25" stroke-width="3" />
  <path d="M21 12a9 9 0 0 0-9-9" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
</svg>

<style>
  .spinner {
    flex: none;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
