<!--
  Code editor (CodeMirror 6, loaded on first use) for compose files and app
  config files. Two-way binds `value`; Ctrl/Cmd+S calls `onsave`.
-->
<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { EditorView } from '@codemirror/view';
  import Spinner from './Spinner.svelte';

  interface Props {
    value: string;
    language?: string;
    readonly?: boolean;
    height?: string;
    label?: string;
    onsave?: () => void;
  }

  let { value = $bindable(''), language = 'text', readonly = false, height = '480px', label = 'Editor', onsave }: Props = $props();

  let host: HTMLDivElement | undefined = $state();
  let view: EditorView | null = null;
  let loading = $state(true);
  let applying = false;

  async function languageExtension(lang: string) {
    const { StreamLanguage } = await import('@codemirror/language');
    switch (lang) {
      case 'yaml':
        return (await import('@codemirror/lang-yaml')).yaml();
      case 'json':
        return (await import('@codemirror/lang-json')).json();
      case 'xml':
        return (await import('@codemirror/lang-xml')).xml();
      case 'ini':
        return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/properties')).properties);
      case 'toml':
        return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/toml')).toml);
      case 'shell':
        return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/shell')).shell);
      case 'dockerfile':
        return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/dockerfile')).dockerFile);
      case 'python':
        return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/python')).python);
      case 'javascript':
        return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/javascript')).javascript);
      default:
        return [];
    }
  }

  async function create() {
    const [{ EditorState }, viewMod, cmds, lang, search, { tags }] = await Promise.all([
      import('@codemirror/state'),
      import('@codemirror/view'),
      import('@codemirror/commands'),
      import('@codemirror/language'),
      import('@codemirror/search'),
      import('@lezer/highlight'),
    ]);
    const langExt = await languageExtension(language);
    if (!host) return;
    const { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection, highlightSpecialChars } = viewMod;

    const style = lang.HighlightStyle.define([
      { tag: [tags.keyword, tags.propertyName, tags.definition(tags.propertyName)], color: 'var(--cm-key)' },
      { tag: [tags.string, tags.special(tags.string)], color: 'var(--cm-string)' },
      { tag: [tags.number, tags.bool, tags.null, tags.atom], color: 'var(--cm-number)' },
      { tag: [tags.comment, tags.lineComment, tags.blockComment], color: 'var(--cm-comment)', fontStyle: 'italic' },
      { tag: [tags.tagName, tags.typeName], color: 'var(--cm-tag)' },
      { tag: [tags.attributeName, tags.variableName], color: 'var(--cm-attr)' },
      { tag: [tags.meta, tags.processingInstruction, tags.punctuation, tags.separator], color: 'var(--cm-punct)' },
      { tag: tags.heading, fontWeight: '600' },
    ]);

    const theme = EditorView.theme({
      '&': { height: '100%', backgroundColor: 'var(--sunken)', color: 'var(--text)', fontSize: '13px' },
      '.cm-scroller': { fontFamily: 'var(--font-code)', lineHeight: '1.6' },
      '.cm-content': { caretColor: 'var(--accent)', padding: '8px 0' },
      '.cm-gutters': { backgroundColor: 'var(--sunken)', color: 'var(--text-3)', border: 'none', borderRight: '1px solid var(--line)' },
      '.cm-activeLineGutter': { backgroundColor: 'var(--raised)', color: 'var(--text-2)' },
      '.cm-activeLine': { backgroundColor: 'color-mix(in srgb, var(--raised) 55%, transparent)' },
      '.cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection': { backgroundColor: 'var(--accent-soft) !important' },
      '.cm-cursor': { borderLeftColor: 'var(--accent)' },
      '&.cm-focused': { outline: 'none' },
      '.cm-matchingBracket': { backgroundColor: 'var(--accent-soft)', outline: '1px solid var(--line-strong)' },
      '.cm-searchMatch': { backgroundColor: 'color-mix(in srgb, var(--accent) 30%, transparent)' },
      '.cm-panels': { backgroundColor: 'var(--panel)', color: 'var(--text)', borderTop: '1px solid var(--line)' },
      '.cm-panels input, .cm-panels button': { fontFamily: 'var(--font-ui)' },
      '.cm-textfield': { backgroundColor: 'var(--sunken)', border: '1px solid var(--line-strong)', color: 'var(--text)', borderRadius: '4px' },
      '.cm-button': { backgroundImage: 'none', backgroundColor: 'var(--raised)', border: '1px solid var(--line-strong)', color: 'var(--text)', borderRadius: '4px' },
    });

    const saveKey = keymap.of([
      {
        key: 'Mod-s',
        preventDefault: true,
        run: () => {
          onsave?.();
          return true;
        },
      },
    ]);

    const state = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightSpecialChars(),
        cmds.history(),
        drawSelection(),
        lang.indentOnInput(),
        lang.bracketMatching(),
        highlightActiveLine(),
        search.highlightSelectionMatches(),
        lang.syntaxHighlighting(style),
        langExt,
        saveKey,
        keymap.of([...cmds.defaultKeymap, ...cmds.historyKeymap, ...search.searchKeymap, cmds.indentWithTab]),
        EditorState.readOnly.of(readonly),
        EditorView.contentAttributes.of({ 'aria-label': label }),
        theme,
        EditorView.updateListener.of((u) => {
          if (u.docChanged && !applying) value = u.state.doc.toString();
        }),
      ],
    });
    view?.destroy();
    view = new EditorView({ state, parent: host });
    loading = false;
  }

  // Recreate when the language or read-only mode changes.
  $effect(() => {
    void language;
    void readonly;
    if (host) void create();
  });

  // Push outside changes (loading a file, restoring a version) into the editor.
  $effect(() => {
    const v = value;
    if (view && v !== view.state.doc.toString()) {
      applying = true;
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: v } });
      applying = false;
    }
  });

  onDestroy(() => view?.destroy());
</script>

<div class="editor" bind:this={host} style:height>
  {#if loading}<div class="loading"><Spinner size={15} /> Loading editor…</div>{/if}
</div>

<style>
  .editor {
    position: relative;
    overflow: hidden;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    --cm-key: #82aaff;
    --cm-string: #c3e88d;
    --cm-number: #f78c6c;
    --cm-comment: var(--text-3);
    --cm-tag: #f07178;
    --cm-attr: #ffcb6b;
    --cm-punct: var(--text-2);
  }
  :global([data-theme='light']) .editor {
    --cm-key: #1f5fbf;
    --cm-string: #2e7d32;
    --cm-number: #c2410c;
    --cm-tag: #b91c1c;
    --cm-attr: #8a5a00;
  }
  .editor:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .loading {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 14px;
    color: var(--text-3);
  }
</style>
