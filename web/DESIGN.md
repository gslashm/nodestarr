# Berth UI design notes

Berth runs a home server's containers the way a terminal runs a quay: every
container has a place, a state and a next move. The UI borrows from that
world on purpose and stays quiet everywhere else.

## Principles

1. **Color means state.** Status colors (running, stopped, paused, failing,
   update available) and the one brand accent are the only saturated colors.
   Panels are flat with hairline borders; shadows only lift overlays.
2. **The yard is the signature.** The dashboard draws containers as
   containers: blocks grouped by stack into bays, colored by state. It is the
   one bold element; everything around it is plain.
3. **One click from the problem.** Wherever something looks wrong, the fix
   (update, restart, logs, restore) is right there. Long actions become tasks
   with a live log in the task tray.
4. **Plain words.** Buttons say what happens ("Update", "Back up now",
   "Restore"). Errors say what failed and what to do next. Sentence case,
   no all-caps labels.
5. **Dense but calm.** Tables use the semi-condensed face with tabular
   figures; rows stay 40px+ tall for touch.

## Tokens

Defined in `src/app.css` as CSS variables, with a dark ("night shift",
default) and a light ("day shift") theme.

| Token | Dark | Light | Use |
|---|---|---|---|
| `--bg` | `#0e151c` harbor ink | `#eef2f5` fog | page |
| `--panel` | `#121b23` quay deck | `#ffffff` | sidebar, panels |
| `--raised` | `#18242e` steel | `#f5f8fa` | hover, inputs on panels |
| `--line` | `#243440` rope | `#d6dee5` | borders |
| `--text` | `#e8eef2` lamp | `#13202a` | primary text |
| `--accent` | `#f2c230` quay yellow | same fill, `#8a6400` text | primary actions, focus, active nav |
| `--ok` | `#38c793` starboard green | `#178a61` | running, healthy |
| `--bad` | `#f05a5a` port red | `#cf3737` | crashed, unhealthy, errors |
| `--update` | `#ff8a3d` signal orange | `#d1621a` | update available |
| `--warn` | `#f5b544` amber | `#b07614` | starting, restarting, warnings |
| `--paused` | `#9aa8ff` periwinkle | `#5462d6` | paused |
| `--idle` | `#6b7d8c` steel | `#7b8d9b` | stopped, created |

App monograms use shipping-line container colors (blue, green, orange,
mustard, navy, rust, teal, magenta, grey), picked by a hash of the app name.

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
