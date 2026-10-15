# Nodestarr UI design notes

Nodestarr is the hub of a star network: every container on the server is a
node, and Nodestarr sits in the middle with a line to each one. The name and
the mark come from that star topology, and the look follows a clear night
sky: deep blue surfaces, quiet lines, one bright accent, and color only
where something needs your attention.

## Principles

1. **Color means state.** Status colors (running, stopped, paused, failing,
   update available) and the one brand accent are the only saturated colors.
   The accent, nebula magenta, was picked because no state uses anything
   near it, so a magenta element is always "Nodestarr" and never "a problem".
   Panels are flat with hairline borders; shadows only lift overlays.
2. **The grid is the signature.** The dashboard draws every container as a
   tile, grouped by stack, with a band in its state color on a faint star
   field. It is the one bold element; everything around it is plain.
3. **One click from the problem.** Wherever something looks wrong, the fix
   (update, restart, logs, restore) is right there. Long actions become tasks
   with a live log in the task tray.
4. **Plain words.** Buttons say what happens ("Update", "Back up now",
   "Restore"). Errors say what failed and what to do next. Sentence case,
   no all-caps labels.
5. **Dense but calm.** Tables use the semi-condensed face with tabular
   figures; rows stay 40px+ tall for touch.

## Mark and wordmark

The mark is a five-point star with a hub-and-spoke network inside: a hub in
the middle and one node toward each point. It is drawn in the accent with
the network in `--accent-ink`, so it follows a custom accent too. The
wordmark is "node" in the text color and "starr" in the accent, set in
Barlow Semi Condensed 700, lowercase, like the *arr apps it sits next to.

Sources: `src/components/Logo.svelte` (in the app), `public/favicon.svg`,
and `public/icons/icon.svg` and `icon-maskable.svg` for the installed app.
The PNG icons are rendered from those SVGs at 192, 512 and 180 (Apple)
pixels.

## Tokens

Defined in `src/app.css` as CSS variables, with a dark (default) and a light
theme.

| Token | Dark | Light | Use |
|---|---|---|---|
| `--bg` | `#0d1220` deep space | `#f1f2f8` | page |
| `--panel` | `#121a2b` night | `#ffffff` | sidebar, panels |
| `--raised` | `#182238` | `#f6f7fb` | hover, inputs on panels, tiles |
| `--line` | `#243049` | `#d9dcea` | borders |
| `--text` | `#e8ecf6` starlight | `#151a2d` | primary text |
| `--accent` | `#e46cff` nebula magenta | same fill, darker text | primary actions, focus, active nav, the mark |
| `--ok` | `#38c793` green | `#178a61` | running, healthy |
| `--bad` | `#f05a5a` red | `#cf3737` | crashed, unhealthy, errors |
| `--update` | `#ff8a3d` orange | `#d1621a` | update available |
| `--warn` | `#f5b544` amber | `#b07614` | starting, restarting, warnings |
| `--paused` | `#9aa8ff` periwinkle | `#5462d6` | paused |
| `--idle` | `#6d7a92` slate | `#7d86a2` | stopped, created |
| `--starfield` | faint white dots | faint ink dots | star-field texture on tiles and the sign-in page |

The accent can be changed in Settings > General. Text and marks drawn on it
use `--accent-ink`, a very dark shade of the accent (or white on a dark
accent), chosen for contrast.

The star field is a small SVG of randomly placed faint dots (seeded, so it
never changes), embedded in `app.css` as a data URI and repeated: 220 by 120
pixels on tiles, 640 pixels square on the sign-in page.

App monograms use muted colors (blue, evergreen, copper, mustard, indigo,
brick, teal, olive, slate) picked by a hash of the app name; none of them
is close to a state color or the accent.

## Type

- **Barlow** (400/500/600) for interface text, 15px base.
- **Barlow Semi Condensed** (500/600/700) for tables, numbers and the
  wordmark, always with tabular figures.
- **IBM Plex Mono** (400/600) only for code: logs, the console, editors.

Scale (major third): 12, 13, 15, 19, 24, 30.

## Layout

*arr-style frame: a left sidebar (brand, navigation, task tray), a sticky
page toolbar (title, actions, search) and a scrolling body. Content is left
aligned. Below 760px the sidebar becomes a drawer, toolbars fold actions into
an overflow menu, and tables turn into stacked rows.
