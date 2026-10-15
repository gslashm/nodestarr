import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';

const app = mount(App, { target: document.getElementById('app')! });

// Installable app (needs HTTPS or localhost; harmless elsewhere).
if ('serviceWorker' in navigator && window.isSecureContext && import.meta.env.PROD) {
  navigator.serviceWorker.register('sw.js').catch(() => {
    // Not fatal: Berth works without it.
  });
}

export default app;
