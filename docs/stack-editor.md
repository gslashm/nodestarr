# Stack editor

Every stack has an **Editor** tab (Stacks > a stack > Editor). It shows the
stack's services as forms, its variables as a list, and all its files
(compose files, `.env`, env files, Dockerfiles), and saves and deploys them
together. The **Edit** tab of a container that belongs to a stack opens the
same form for just that service.

The forms and the files are two views of the same text: change a setting
and the YAML changes; change the YAML and the form follows. Edits keep the
files the way you wrote them, with their comments, quotes, key order and
blank lines. Only the lines a change touches are rewritten.

## Services

Each service has sections:

| Section | What's in it |
|---|---|
| General | The image (with a tag picker, and a card with what the image sets up by itself) or the Dockerfile it's built from, the container name, the restart policy, the command, and the services it starts after. |
| Environment | All its environment variables, from every source (see below). |
| Ports | Published ports, with a warning when another service or container already uses a host port, and ports for other containers only. |
| Storage | Folders, volumes and memory folders, with the full path each folder ends up at. Named volumes are declared at the top of the file when you first use them. |
| Network | The network mode, the networks it joins (ones that already exist on the server are joined, not created), hostname, DNS and extra hosts. |
| Health | The image's own check, a custom one (with ready-made ones for web pages, ports and PostgreSQL), or none. |
| Resources | Memory, CPU and process limits, NVIDIA GPUs, devices, extra groups and shared memory. |
| Advanced | The name and address Nodestarr shows, user, working folder, entrypoint, switches like init and read-only, capabilities, kernel settings, labels, logging, stopping, pull policy, platform and profiles. |
| Other | Everything else (ulimits, secrets, extends, `x-` keys...) as YAML you can edit, remove or add to. |

The **YAML** button shows the service as written next to the form: changed
lines are marked, and the lines of the section you're in are highlighted.

The image card suggests what's missing: "Publish 8989/tcp" for ports the
image listens on, "Keep /config in a folder" for the folders it stores data
in. Nodestarr reads this from the image on the server, or from its registry
without pulling it.

Services that share settings through a YAML anchor (`<<: *common`) show
the shared values. Changing one gives that service its own copy; the
anchor and the other services stay as they are. When an override file
(`compose.override.yaml`) also sets something for a service, the form says
so, since the override wins.

Rename, duplicate or remove a service from its menu. Renaming also updates
the services that start after it.

## Environment

A container gets its environment from four places, and the Environment
section shows them as one list, in the order compose applies them (later
ones win):

1. the image's own variables (shown with **Show ... from the image**),
2. env files (`env_file:`), in order,
3. the service's `environment:`, where `${NAME}` takes `NAME` from the
   stack's `.env`.

Each variable shows where it comes from (`compose.yaml`, `.env · NAME`, an
env file, or the image), what it overrides, and the value the container
ends up with ("becomes ..."). Editing a value changes it where it lives:

| The variable is written as | Editing it changes |
|---|---|
| `TZ: Europe/Berlin` | the compose file |
| `TZ: ${TZ}` or a plain `- TZ` | `TZ` in `.env` (and every service that uses it) |
| a line in an env file | that env file |
| from the image | adds an override in the compose file |

Values that look like passwords or tokens are hidden until you show them.
**Keep the value in .env** moves a value out of the compose file into
`.env` and leaves `${NAME}` in its place, so the compose file can be shared
without its secrets. New variables with secret-looking names go to `.env`
by default.

Warnings point out variables that aren't set, `${NAME:?...}` that would stop
compose, and a lone `$` (compose reads `$word` as a variable; write `$$` for
a literal dollar sign, or use **keep "$word" as text**). When the running
container has a different value, or a variable that's no longer in the
files, the list says so: deploy to apply it.

Paste a whole `.env` file or the `-e` flags of a `docker run` command with
**Paste variables**, and copy the result as a `.env` file or as `docker run`
flags.

## Variables

**Variables** is the stack's `.env` as a list. Each variable shows which
services use it, and comment lines right above a variable become its
description. The list also shows the variables compose needs that aren't set
yet (with a button to generate a strong password for secret ones), the
ones that fall back to a default, and the ones nothing uses. Renaming a
variable renames its `${...}` references in the compose files too.

Env files open in the same list, and every list can be edited as text.

## Dockerfiles and other files

Services built from a Dockerfile (General > Runs > A Dockerfile) have their
build folder, Dockerfile, target stage and build arguments there. Switching
a service from an image to a Dockerfile starts one from that image.

Dockerfiles open as cards, one per instruction, grouped by build stage, with
fields that suit each instruction (FROM: image and stage name, COPY: sources,
destination and options, ENV: a list...). Drag a card by its handle or press
Alt+↑/↓ inside it to move it; add instructions between cards or at the end.
Suggestions point out common mistakes, like an unpinned base image, running
as root, or a CMD that won't receive stop signals. Empty Dockerfiles start
from a template (Node.js, Python, Go, a static website, or extending an
image), and `.dockerignore` files open as a list of patterns.

**Open a file** (Files > the file icon) opens any text file in the stack's
folder, or creates one. Files in the stacks folders set in Settings > Docker
can be opened too, for env files several stacks share.

## New stacks

Stacks > Create stack starts the same editor for a new folder in the first
stacks folder. Add services from:

- **Apps**: common self-hosted apps with sensible defaults. You pick the
  service name and host ports (free ones are suggested), and the stack
  variables it uses (`PUID`, `PGID`, `TZ`, media folders) go into `.env`.
  Passwords are generated. Some apps bring files they need, like
  Mosquitto's configuration.
- **docker run**: paste a command from an app's documentation.
- **A container**: bring in a container you started by hand, with its
  settings. Remove the old container before deploying, or they clash.
- **Blank**: a name and an image.

Or use **Paste a compose file instead**.

## Saving

The bar at the bottom lists the files you changed:

- **Check** runs `docker compose config` on the edited files.
- **Review** shows every change as a diff, file by file.
- **Save** writes the files (compose checks them first) and keeps the
  previous version of each under History.
- **Save and deploy** also runs `docker compose up -d`, rebuilding images
  when a Dockerfile changed. On a container's page this recreates just that
  container.

Ctrl+S saves. Ctrl+Z and Ctrl+Shift+Z undo and redo across all the files
(inside a text field they undo the typing). If the files change on the
server while you edit, you're told before saving over them.

The **Overview** lists the services and what looks wrong before compose
does: volumes or networks used but not declared (with a fix), ports two
services publish, services that start after one that doesn't exist, and
variables that aren't set.

The address follows what you're looking at
(`/stacks/media/compose?service=sonarr&section=environment`), so you can
bookmark or share it.
