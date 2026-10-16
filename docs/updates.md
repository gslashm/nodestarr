# Updates

Nodestarr checks the registries your images come from, tells you what is
new, installs updates when you click Update (or on its own, if you allow
it), and undoes them when something goes wrong.

## How checks work

Every 6 hours by default (Settings > Updates > Check schedule), and when you
click Check now, Nodestarr asks each image's registry two questions:

1. **Is there a new build of the tag I run?** It compares the digest of the
   image you run with the tag's current digest. This is how `latest`,
   `develop` or `stable` updates show up. The check uses `HEAD` requests,
   which Docker Hub doesn't count against its pull limits, and each image is
   only asked once even when several containers use it.
2. **Is there a newer version tag?** For tags that look like versions
   (`5.28.0.10274`, `v2.1.0`, `16-alpine`), it lists the repository's tags
   and finds the newest one of the same kind. Settings > Updates > Report
   new tags decides how far to look (patch, minor or major), and
   pre-releases (alpha, beta, rc, nightly) are skipped unless you turn them
   on.

When a new build exists, Nodestarr reads its version from the image labels
(the OCI version label, LinuxServer's build version, or the app's version
variable), so you see "5.0.0.2700" instead of a digest. Release notes come
from the app's GitHub releases, and decide together with the versions what
type of update it is (see [Update types](#update-types)).

Registries: Docker Hub, GitHub (ghcr.io), LinuxServer (lscr.io), Quay,
Google, and any registry that speaks the standard API. Add credentials for
private registries in Settings > Updates > Registry logins (matched by
host, for example `ghcr.io`). A GitHub token there raises GitHub's rate
limit for release notes.

A container shows one of these states:

| State | Meaning |
|---|---|
| Up to date | You run the newest build of your tag. |
| Update | A new build or a newer version tag is out. |
| Pulled, recreate to apply | The new image is already on the machine; recreating the container applies it. |
| Check failed | The registry couldn't be reached or refused; the error says why. |
| Local image | Built on this machine, nothing to compare with. |
| Pinned | The image is pinned by digest (`image@sha256:...`) and never changes. |
| Not checked | Its policy is Ignore. |

## Update types

Every update gets a type, shown as chips on the Updates page, the
container's Updates tab and the dashboard. The Updates page can show one
type at a time (the filter is in the address, so `/updates?type=security`
can be bookmarked) and sorts by type by default, most important first.

How far the version moves:

| Type | Meaning |
|---|---|
| Major | The first number changes (1.x to 2.0). |
| Minor | The second number changes (1.2 to 1.3). |
| Patch | A later number changes (1.2.3 to 1.2.4, or Sonarr's 4.0.9.2244 to 4.0.10.2544). |
| Rebuild | The same version in a new image: a refreshed base image, or a packaging revision such as LinuxServer's `ls250` to `ls251`. |
| Unknown | The image doesn't say which version it is, so the size can't be told. |

Only the numbers count, so `v1.2` and `1.3-alpine` compare fine.
Year-based versions (Home Assistant's 2024.12 to 2025.1) are never major:
a change of year or month is minor. For a new build of a tag like `latest`,
the running version and the new one are read from the image labels or,
for official images such as nginx or postgres, from their version variable
(`NGINX_VERSION`). When neither says, the tag still limits the change: a
new build of `16` is at most minor, of `16.4` at most a patch. Such
estimates are shown as "≤ Minor" with a dashed outline.

What the release notes say, on top of the size:

| Type | Meaning |
|---|---|
| Security | The release notes mention security fixes: a CVE or GitHub advisory id, a "Security" section, "security fix", a fixed vulnerability, or a named attack such as XSS or SQL injection. |
| Breaking | The release notes mention breaking changes: "breaking change", a "Breaking changes" section, "backwards incompatible", "action required", "manual intervention", or a conventional-commit `feat!:`. "No breaking changes" and "Breaking changes: none" don't count. |

Nodestarr reads every release after the version you run, up to the one the
update brings, so a breaking change two releases back still counts. The
line that gave it away is shown on the Updates page and the container's
Updates tab, with the release it's in. Release notes come from GitHub (the
30 newest releases of the app's repository); when they don't reach back to
your version, the Updates tab says so. Images without a known GitHub
repository get a size but no security or breaking marks.

This is read from free text, so it can be wrong. It errs towards calling an
update breaking, which only holds automatic updates back, and is strict
about security, which can let them install.

## Policies

Each container has an update policy (Settings > Updates sets the default for
containers you haven't configured):

- **Ignore**: no checks.
- **Tell me** (default): check and notify, never install on its own.
- **Automatic**: install during the automatic update window (04:00 by
  default, Settings > Updates > Automatic updates).

Automatic updates have more limits:

- **Which types install.** Settings > Updates > Install on their own, or
  per container in its settings. By default rebuilds, patch and minor
  versions and updates of unknown size install; major versions wait for
  you. Security fixes can install even when their size isn't ticked
  (never major versions). Updates whose release notes mention breaking
  changes wait for you, unless you tick Breaking.
- **Whether tags change.** For containers on a version tag, Switch to
  newer tags lets automatic updates move `5.28` to `5.29` when that
  version's type is ticked. It's off by default, so only new builds of the
  tag you run are installed. When a newer tag isn't allowed but the
  current tag has a new build, that build is installed instead.
- **How old an update must be.** Updates are only installed once they have
  been out for at least 6 hours (configurable, also per container), so a
  broken release has time to be pulled or fixed. Clicking Update ignores
  this.

The Updates page says for each update whether it installs on its own and
when, or why it waits for you.

You can also **hold** a container (Container > Updates > Hold), which pauses
automatic updates for it until you release it, and **skip** a build, which
stops offering that particular build.

The Plex guard (on by default) postpones automatic updates and restarts of
Plex while someone is streaming, using Plex's or Tautulli's API.

## What an update does

1. **Backs up** the container first, when it has backups enabled and
   Settings > Updates > Back up before updating is on. If that backup fails,
   the update stops. (The confirmation dialog lets you skip the backup.)
2. **Keeps the current image** under a `nodestarr-rollback/...` tag, so it
   can't be cleaned up while it may still be needed.
3. **Pulls** the new image.
4. **Recreates** the container. For Compose stacks Nodestarr runs
   `docker compose up -d` for that service, so the stack's own definition is
   used; for a version change it first edits the image tag in the compose
   file or in `.env` when the tag comes from a variable
   (`image: ghcr.io/x/app:${APP_TAG}`). Standalone containers are recreated
   with exactly their old settings. Containers that share its network
   (`network_mode: container:...`, like apps behind Gluetun) are recreated
   too.
5. **Verifies** that the new container keeps running for 15 seconds and, if
   it has a health check, becomes healthy, within 3 minutes.
6. **Rolls back** when verification fails: the previous image and compose
   file come back, the container is recreated from them, you get a
   notification with the reason, and automatic updates skip that build.
7. **Cleans up**: only the newest 2 rollback images are kept per container,
   and dangling images are removed.

Every update is recorded in the update history (Updates > History and on
each container), with its task log. While a rollback image is still kept,
**Roll back** returns the container to the version before that update.

Bulk updates (select several on the Updates page) run one after another.

## Changing the tag or branch

Container > Updates > Change version lists the repository's tags, newest
first. Pick one, for example `develop` to move Sonarr to v5, or a specific
version to go back; Nodestarr shows the exact change it will make to the
compose file (or `.env`) before switching. The same backup, verification and
rollback apply.

## Updating Nodestarr itself

See [Installation](installation.md#updating-nodestarr). Automatic updates
update Nodestarr last, since that restarts it.
