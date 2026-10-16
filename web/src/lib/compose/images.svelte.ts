// Image details for the stack editor (what an image sets up by itself, and
// its tags), fetched once per image and shared by every field that shows them.

import { api, errorMessage } from '../api';
import type { ImageInspect, ImageTags } from '../types';
import { variables } from './imageref';

export type Loaded<T> = { state: 'loading' } | { state: 'ok'; data: T } | { state: 'error'; message: string };

class Images {
  inspect = $state<Record<string, Loaded<ImageInspect>>>({});
  tags = $state<Record<string, Loaded<ImageTags>>>({});

  /** Starts loading an image's details; call from an effect, then read `inspect`. */
  loadInspect(image: string): void {
    const key = image.trim();
    if (!key || variables(key).length || this.inspect[key]) return;
    this.inspect[key] = { state: 'loading' };
    api
      .get<ImageInspect>(`/images/inspect?image=${encodeURIComponent(key)}`)
      .then((data) => (this.inspect[key] = { state: 'ok', data }))
      .catch((err) => (this.inspect[key] = { state: 'error', message: errorMessage(err) }));
  }

  loadTags(image: string): void {
    const key = image.trim();
    if (!key || variables(key).length || this.tags[key]) return;
    this.tags[key] = { state: 'loading' };
    api
      .get<ImageTags>(`/images/tags?image=${encodeURIComponent(key)}`)
      .then((data) => (this.tags[key] = { state: 'ok', data }))
      .catch((err) => (this.tags[key] = { state: 'error', message: errorMessage(err) }));
  }

  /** The image's ENV as key/value pairs, once loaded. */
  env(image: string): { key: string; value: string }[] | null {
    const e = this.inspect[image.trim()];
    if (e?.state !== 'ok') return null;
    return e.data.defaults.env.map((kv) => {
      const i = kv.indexOf('=');
      return i < 0 ? { key: kv, value: '' } : { key: kv.slice(0, i), value: kv.slice(i + 1) };
    });
  }
}

export const images = new Images();
