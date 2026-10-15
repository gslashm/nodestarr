import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// During development the UI runs on Vite's server and talks to a Nodestarr
// instance (default http://127.0.0.1:7979, override with NODESTARR_DEV_API).
const api = process.env.NODESTARR_DEV_API ?? 'http://127.0.0.1:7979';

export default defineConfig({
  plugins: [svelte()],
  // Relative asset URLs, so the UI works under any URL base (<base href> is
  // injected by the server).
  base: './',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
    cssCodeSplit: true,
    assetsInlineLimit: 0,
    chunkSizeWarningLimit: 700,
  },
  server: {
    port: 5173,
    proxy: {
      '/api': { target: api, ws: true, changeOrigin: false },
      '/metrics': { target: api },
    },
  },
});
