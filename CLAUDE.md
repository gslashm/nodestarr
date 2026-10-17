# Working on Nodestarr

Rust server (`src/`) plus a Svelte 5 web UI (`web/`). Layout, conventions and
the full list of checks are in [docs/development.md](docs/development.md);
UI design rules in [web/DESIGN.md](web/DESIGN.md).

## Where to run what (Windows machine with WSL)

Windows Rust can't link here (no Windows SDK), and parts of the server are
Unix-only. Run every Rust command in WSL (Ubuntu 24.04, rustup Rust, Node 22),
from the Windows checkout, with the build output in the Linux file system
(fast; doesn't touch `target/` on the Windows side):

```sh
wsl -e bash -lc 'cd /mnt/c/Users/GslshM/Downloads/nodestarr && export CARGO_TARGET_DIR=$HOME/.cache/nodestarr-target && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked'
```

Run the web checks on Windows (`web/node_modules` holds Windows binaries),
from `web/`: `npm run check`, `npm test`, `npm run build`.

WSL can restart between commands, which clears `/tmp`: keep anything that
must survive in the home folder or the scratchpad.

## Trying changes without real hardware

- `dev/` has stand-ins for Proxmox (`pve-mock.py`), the disk host helper
  (`host-helper.py`, with `fakedisks.py`) and webhooks; see dev/README.md.
- `bash dev/smoke-disks.sh` (in WSL) builds Nodestarr and runs it against
  the Proxmox mock and the helper stand-in, without Docker: it reads all
  disks and pools and runs a self-test. Its approach (a dummy Docker socket,
  `NODESTARR_ADMIN_USER`/`_PASSWORD`, signing in with a cookie jar and the
  `X-Nodestarr-Request: 1` header) works for trying other API features too.
- No Docker on this machine yet, so the container image isn't built locally;
  CI builds it.

## Habits

- Commit only when asked. The main branch is `main`.
- Keep docs (`docs/`, `CHANGELOG.md` under Unreleased) in step with
  user-visible changes.
