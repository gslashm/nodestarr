// The stack editor's working copy: every file of a stack (compose files,
// .env, env files, Dockerfiles...) with its saved and edited text, the
// parsed documents the forms read, and undo/redo across all of them.
//
// Text is the source of truth. Visual edits parse the current text, change
// the document and write it back keeping the layout (see yamldoc.ts), so
// the code view and the forms always show the same thing.

import { getContext, setContext } from 'svelte';
import { api, seg } from '../api';
import type { StackFile } from '../types';
import { evaluateDotenv } from './dotenv';
import { allServices, envFilesOf, variableUsage } from './environment';
import { editService, serviceMap } from './service';
import { editYaml, parseYaml, yamlError, type YAMLMap, type YDoc } from './yamldoc';

export type FileKind = 'compose' | 'env' | 'env_file' | 'dockerfile' | 'dockerignore' | 'other';

export interface WorkFile {
  /** Path relative to the stack folder, or as written in compose when it's elsewhere. */
  rel: string;
  /** Absolute path on the server ('' until the stack exists). */
  abs: string;
  kind: FileKind;
  /** Content on disk; null when the file doesn't exist yet. */
  original: string | null;
  text: string;
  loading: boolean;
  /** Why it couldn't be opened (outside the stack folder, too large...). */
  error: string;
}

export type Selection =
  | { kind: 'stack' }
  | { kind: 'service'; name: string; section?: string }
  | { kind: 'variables' }
  | { kind: 'file'; rel: string };

interface Snapshot {
  files: Record<string, WorkFile>;
  label: string;
  at: number;
}

/** Joins and normalizes relative paths ("./a/../b" → "b"); keeps leading "..". */
export function joinPath(...parts: string[]): string {
  const out: string[] = [];
  const abs = parts.find((p) => p)?.startsWith('/') ?? false;
  for (const part of parts) {
    if (part.startsWith('/')) out.length = 0;
    for (const seg of part.split('/')) {
      if (!seg || seg === '.') continue;
      if (seg === '..' && out.length && out[out.length - 1] !== '..') out.pop();
      else out.push(seg);
    }
  }
  const joined = out.join('/');
  return abs || parts.some((p) => p.startsWith('/')) ? `/${joined}` : joined;
}

export function fileKind(rel: string): FileKind {
  const base = rel.slice(rel.lastIndexOf('/') + 1).toLowerCase();
  if (base === '.dockerignore' || base.endsWith('.dockerignore')) return 'dockerignore';
  if (base === 'dockerfile' || base.startsWith('dockerfile.') || base.endsWith('.dockerfile') || base === 'containerfile') return 'dockerfile';
  if (base === '.env' || base.endsWith('.env') || base.startsWith('.env.')) return 'env_file';
  if (/(^|\.)(docker-)?compose(\.[\w-]+)?\.ya?ml$/.test(base)) return 'compose';
  return 'other';
}

/** Where a service's Dockerfile and .dockerignore live, from its `build:` setting. */
export function buildFiles(build: unknown): { context: string; dockerfile: string; ignore: string; inline: boolean } | null {
  if (build === undefined || build === null) return null;
  if (typeof build === 'string') {
    const context = joinPath(build) || '.';
    return { context, dockerfile: joinPath(context, 'Dockerfile'), ignore: joinPath(context, '.dockerignore'), inline: false };
  }
  if (typeof build === 'object') {
    const b = build as Record<string, unknown>;
    const context = joinPath(String(b.context ?? '.')) || '.';
    if (/^[a-z]+:\/\//i.test(context) || context.startsWith('git@')) return null;
    const df = String(b.dockerfile ?? 'Dockerfile');
    return {
      context,
      dockerfile: df.startsWith('/') ? df : joinPath(context, df),
      ignore: joinPath(context, '.dockerignore'),
      inline: typeof b.dockerfile_inline === 'string',
    };
  }
  return null;
}

const COALESCE_MS = 900;
const MAX_HISTORY = 200;

export class Studio {
  /** Stack name (for new stacks: the name being typed). */
  name = $state('');
  readonly isNew: boolean;
  /** Absolute stack folder ('' until known). */
  dir = $state('');
  files = $state<Record<string, WorkFile>>({});
  /** Compose files in the order compose reads them. */
  composeRels = $state<string[]>([]);
  /** The stack's own variables file. */
  envRel = $state('.env');
  selection = $state<Selection>({ kind: 'stack' });
  loadError = $state('');
  loaded = $state(false);
  /** What Ctrl+S does (set by the page hosting the editor). */
  onsave: (() => void) | null = null;

  private past = $state<Snapshot[]>([]);
  private future = $state<Snapshot[]>([]);

  constructor(name: string, isNew = false) {
    this.name = name;
    this.isNew = isNew;
  }

  // ------------------------------------------------------------ derived

  /** Parsed compose files, in order. */
  docs = $derived.by(() =>
    this.composeRels
      .map((rel) => this.files[rel])
      .filter((f): f is WorkFile => !!f)
      .map((f) => ({ file: f.rel, doc: parseYaml(f.text) })),
  );

  /** Compose files that don't parse, with the first error. */
  yamlErrors = $derived.by(() => {
    const out: Record<string, string> = {};
    for (const { file, doc } of this.docs) {
      const e = yamlError(doc);
      if (e) out[file] = e;
    }
    return out;
  });

  services = $derived(allServices(this.docs));

  /** The stack's variables (.env), evaluated. */
  project = $derived(evaluateDotenv(this.files[this.envRel]?.text ?? ''));

  usage = $derived(variableUsage(this.docs));

  /** Files whose text differs from what's on disk. */
  changes = $derived(
    Object.values(this.files).filter((f) => !f.loading && !f.error && (f.original === null ? f.text !== '' : f.text !== f.original)),
  );

  canUndo = $derived(this.past.length > 0);
  canRedo = $derived(this.future.length > 0);
  undoLabel = $derived(this.past[this.past.length - 1]?.label ?? '');
  redoLabel = $derived(this.future[this.future.length - 1]?.label ?? '');

  // ------------------------------------------------------------- loading

  /** Loads an existing stack's compose and .env files, then what they refer to. */
  async load(): Promise<void> {
    try {
      const r = await api.get<{ files: StackFile[]; dir: string }>(`/stacks/${seg(this.name)}/files`);
      this.dir = r.dir;
      const files: Record<string, WorkFile> = {};
      const compose: string[] = [];
      for (const f of r.files) {
        const rel = f.name;
        files[rel] = { rel, abs: f.path, kind: f.kind === 'env' ? 'env' : 'compose', original: f.exists ? f.content : null, text: f.content, loading: false, error: '' };
        if (f.kind === 'compose') compose.push(rel);
        else this.envRel = rel;
      }
      this.files = files;
      this.composeRels = compose;
      this.past = [];
      this.future = [];
      this.loadError = '';
      this.loaded = true;
      void this.discover();
    } catch (err) {
      this.loadError = err instanceof Error ? err.message : String(err);
    }
  }

  /**
   * Checks the compose and .env files on the server against what the editor
   * was opened with. Without unsaved changes the editor takes the new
   * content; with them it's a conflict for the user to settle.
   */
  async checkServer(): Promise<'same' | 'reloaded' | 'conflict'> {
    if (this.isNew || !this.loaded) return 'same';
    const r = await api.get<{ files: StackFile[]; dir: string }>(`/stacks/${seg(this.name)}/files`);
    const differs = r.files.filter((f) => this.files[f.name]?.original !== (f.exists ? f.content : null));
    if (!differs.length) return 'same';
    // Our own save landing before its response: the server has exactly what's being edited.
    if (differs.every((f) => f.exists && this.files[f.name]?.text === f.content)) {
      for (const f of differs) this.files[f.name].original = f.content;
      return 'same';
    }
    if (!this.changes.length) {
      const selection = this.selection;
      await this.load();
      this.selection = selection;
      return 'reloaded';
    }
    return 'conflict';
  }

  /** Starts an empty stack. */
  startNew(dir: string, compose = 'services:\n', env = ''): void {
    this.dir = dir;
    this.files = {
      'compose.yaml': { rel: 'compose.yaml', abs: '', kind: 'compose', original: null, text: compose, loading: false, error: '' },
      '.env': { rel: '.env', abs: '', kind: 'env', original: null, text: env, loading: false, error: '' },
    };
    this.composeRels = ['compose.yaml'];
    this.envRel = '.env';
    this.loaded = true;
  }

  /** Loads files the compose files refer to (env files, Dockerfiles, .dockerignore). */
  async discover(): Promise<void> {
    const wanted: [string, FileKind][] = [];
    for (const { doc } of this.docs) {
      for (const name of allServices([{ doc }])) {
        for (const ref of envFilesOf(doc, name)) wanted.push([joinPath(ref.path), 'env_file']);
        const svc = serviceMap(doc, name);
        const b = svc ? buildFiles(svc.toJS(doc)?.build) : null;
        if (b && !b.inline) {
          wanted.push([b.dockerfile, 'dockerfile']);
          wanted.push([b.ignore, 'dockerignore']);
        }
      }
    }
    await Promise.all(wanted.filter(([rel]) => !this.files[rel]).map(([rel, kind]) => this.open(rel, kind)));
  }

  /** Opens a file of the stack folder (missing files open empty, to be created on save). */
  async open(rel: string, kind: FileKind = fileKind(rel)): Promise<WorkFile> {
    const existing = this.files[rel];
    if (existing) return existing;
    this.files[rel] = { rel, abs: this.dir ? joinPath(this.dir, rel) : '', kind, original: null, text: '', loading: !this.isNew, error: '' };
    if (this.isNew) return this.files[rel];
    try {
      const r = await api.get<{ path: string; content: string; exists: boolean; error?: string }>(`/stacks/${seg(this.name)}/file?path=${encodeURIComponent(rel)}`);
      const f = this.files[rel];
      if (f) {
        f.abs = r.path;
        f.loading = false;
        f.error = r.error ?? '';
        // Keep anything typed while it loaded.
        if (r.exists) {
          f.original = r.content;
          if (f.text === '') f.text = r.content;
        }
      }
    } catch (err) {
      const f = this.files[rel];
      if (f) {
        f.loading = false;
        f.error = err instanceof Error ? err.message : String(err);
      }
    }
    return this.files[rel];
  }

  // ------------------------------------------------------------- reading

  doc(rel: string): YDoc | null {
    return this.docs.find((d) => d.file === rel)?.doc ?? null;
  }

  /** The compose file to edit a service in: the first that defines it. */
  serviceFile(name: string): string {
    for (const { file, doc } of this.docs) if (serviceMap(doc, name)) return file;
    return this.composeRels[0] ?? 'compose.yaml';
  }

  /** Later compose files that also change this service (they win). */
  overridesOf(name: string): { file: string; keys: string[] }[] {
    const first = this.serviceFile(name);
    const out: { file: string; keys: string[] }[] = [];
    let seen = false;
    for (const { file, doc } of this.docs) {
      if (file === first) {
        seen = true;
        continue;
      }
      const svc = seen ? serviceMap(doc, name) : null;
      if (svc) out.push({ file, keys: (svc.items as { key: { value?: unknown } }[]).map((p) => String(p.key?.value ?? '')).filter(Boolean) });
    }
    return out;
  }

  text(rel: string): string {
    return this.files[rel]?.text ?? '';
  }

  /**
   * Contents of a service's env files, by the path written in compose:
   * undefined while loading, null when missing or unreadable.
   */
  envFileTexts(service: string): Record<string, string | null | undefined> {
    const out: Record<string, string | null | undefined> = {};
    for (const { doc } of this.docs) {
      for (const ref of envFilesOf(doc, service)) {
        const f = this.files[joinPath(ref.path)];
        out[ref.path] = !f || f.loading ? undefined : f.error || (f.original === null && f.text === '') ? null : f.text;
      }
    }
    return out;
  }

  // ------------------------------------------------------------- editing

  private snapshot(label: string): Snapshot {
    const files: Snapshot['files'] = {};
    for (const [rel, f] of Object.entries(this.files)) files[rel] = { ...f };
    return { files, label, at: Date.now() };
  }

  private record(label: string): void {
    const last = this.past[this.past.length - 1];
    // Typing in one field is one step, not one per key.
    if (last && last.label === label && Date.now() - last.at < COALESCE_MS) {
      last.at = Date.now();
      return;
    }
    this.past = [...this.past.slice(-MAX_HISTORY + 1), this.snapshot(label)];
    this.future = [];
  }

  /** Replaces a file's text (code view, env files, Dockerfiles). */
  setText(rel: string, text: string, label = `Edit ${rel}`): void {
    const f = this.files[rel];
    if (!f || f.text === text) return;
    this.record(label);
    f.text = text;
  }

  /** Changes a compose file through its parsed document. Throws on YAML errors. */
  editCompose(rel: string, mutate: (doc: YDoc) => void, label: string): void {
    const f = this.files[rel];
    if (!f) throw new Error(`${rel} isn't open`);
    this.setText(rel, editYaml(f.text, mutate), label);
  }

  /** Changes one service, in the file that defines it. */
  editService(name: string, mutate: (doc: YDoc, svc: YAMLMap) => void, label: string): void {
    this.editCompose(
      this.serviceFile(name),
      (doc) => {
        const svc = editService(doc, name);
        if (svc) mutate(doc, svc);
      },
      label,
    );
  }

  /** Adds a new file (it's created when saving). */
  addFile(rel: string, content: string, kind: FileKind = fileKind(rel)): void {
    const existing = this.files[rel];
    if (existing) {
      this.setText(rel, content, `Edit ${rel}`);
      return;
    }
    this.record(`Add ${rel}`);
    this.files[rel] = { rel, abs: this.dir ? joinPath(this.dir, rel) : '', kind, original: null, text: content, loading: false, error: '' };
  }

  private restore(s: Snapshot): void {
    for (const [rel, f] of Object.entries(this.files)) {
      const saved = s.files[rel];
      if (saved) f.text = saved.text;
      // Added after the snapshot and never saved: it goes away again.
      else if (f.original === null) delete this.files[rel];
    }
    for (const [rel, saved] of Object.entries(s.files)) if (!this.files[rel]) this.files[rel] = { ...saved };
  }

  undo(): void {
    const s = this.past[this.past.length - 1];
    if (!s) return;
    const now = this.snapshot(s.label);
    this.past = this.past.slice(0, -1);
    this.future = [...this.future, now];
    this.restore(s);
  }

  redo(): void {
    const s = this.future[this.future.length - 1];
    if (!s) return;
    const now = this.snapshot(s.label);
    this.future = this.future.slice(0, -1);
    this.past = [...this.past, now];
    this.restore(s);
  }

  /** Puts one file back the way it is on disk. */
  revert(rel: string): void {
    const f = this.files[rel];
    if (!f) return;
    this.record(`Undo changes to ${rel}`);
    if (f.original === null) delete this.files[rel];
    else f.text = f.original;
  }

  // -------------------------------------------------------------- saving

  /** Changed files for the save request (absolute paths). */
  payload(): { path: string; content: string }[] {
    return this.changes.map((f) => ({ path: f.abs || joinPath(this.dir, f.rel), content: f.text }));
  }

  /** After saving: what's on disk now matches the editor. */
  markSaved(): void {
    for (const f of this.changes) f.original = f.text;
  }

  /** Files of a new stack besides compose.yaml and .env (relative paths). */
  extraFiles(): { path: string; content: string }[] {
    return Object.values(this.files)
      .filter((f) => f.rel !== 'compose.yaml' && f.rel !== this.envRel && f.text !== '')
      .map((f) => ({ path: f.rel, content: f.text }));
  }

  // -------------------------------------------------------------- selection

  select(sel: Selection): void {
    this.selection = sel;
  }

  requestSave(): void {
    this.onsave?.();
  }
}

const KEY = Symbol('studio');

/** Makes a studio available to the editor components below. */
export function provideStudio(s: Studio): Studio {
  return setContext(KEY, s);
}

export function useStudio(): Studio {
  const s = getContext<Studio | undefined>(KEY);
  if (!s) throw new Error('useStudio() outside a stack editor');
  return s;
}
