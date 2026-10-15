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
   newer versions up to decides how far to look (patch, minor or major), and
   pre-releases (alpha, beta, rc, nightly) are skipped unless you turn them
   on.

When a new build exists, Nodestarr reads its version from the image labels
(the OCI version label, LinuxServer's build version, or the app's version
variable), so you see "5.0.0.2700" instead of a digest. Release notes come
from the app's GitHub releases.

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

## Policies

Each container has an update policy (Settings > Updates sets the default for
containers you haven't configured):

- **Ignore**: no checks.
- **Tell me** (default): check and notify, never install on its own.
- **Automatic**: install during the automatic update window (04:00 by
  default, Settings > Updates > Automatic updates).

Automatic updates have two more limits:

- **How far they may go.** For containers on a version tag, choose new
  builds only (the default, so `5.28` never becomes `5.29` by itself), patch,
  minor or major versions.
- **How old an update must be.** Updates are only installed once they have
  been out for at least 6 hours (configurable, also per container), so a
  broken release has time to be pulled or fixed. Clicking Update ignores
  this.

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
