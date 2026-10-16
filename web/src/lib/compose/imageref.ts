// Image references ("lscr.io/linuxserver/sonarr:latest") and how to point
// one at another tag when the tag lives in a variable. Mirrors the server's
// yaml_edit::plan_tag_edit, so the editor and tag switches agree.

import { references } from './interpolate';

export { references as variables };

export interface ImageParts {
  /** Everything before the tag (registry and repository). */
  repo: string;
  tag: string;
  digest: string;
}

export function splitImage(ref: string): ImageParts {
  const at = ref.indexOf('@');
  const digest = at >= 0 ? ref.slice(at + 1) : '';
  const name = at >= 0 ? ref.slice(0, at) : ref;
  const slash = name.lastIndexOf('/');
  const colon = name.lastIndexOf(':');
  if (colon > slash) return { repo: name.slice(0, colon), tag: name.slice(colon + 1), digest };
  return { repo: name, tag: '', digest };
}

export function joinImage(p: ImageParts): string {
  return `${p.repo}${p.tag ? `:${p.tag}` : ''}${p.digest ? `@${p.digest}` : ''}`;
}

/** The short name people use: "sonarr" for "lscr.io/linuxserver/sonarr:latest". */
export function imageBase(ref: string): string {
  const { repo } = splitImage(ref);
  return repo.slice(repo.lastIndexOf('/') + 1);
}

export type TagEdit =
  /** Write this image value in the compose file. */
  | { kind: 'literal'; value: string }
  /** Set this variable (in .env) to the tag. */
  | { kind: 'var'; name: string; value: string }
  /** The whole reference is in this variable; change its tag. */
  | { kind: 'image-var'; name: string };

/** Works out how to point an image value at another tag. */
export function planTagEdit(raw: string, tag: string): TagEdit | null {
  const vars = references(raw);
  if (!vars.length) {
    const p = splitImage(raw);
    return { kind: 'literal', value: joinImage({ repo: p.repo, tag, digest: '' }) };
  }
  if (vars.length === 1) {
    const v = vars[0];
    if (v.start === 0 && v.end === raw.length) return { kind: 'image-var', name: v.name };
    if (v.end === raw.length && raw.slice(0, v.start).endsWith(':')) return { kind: 'var', name: v.name, value: tag };
  }
  return null;
}
