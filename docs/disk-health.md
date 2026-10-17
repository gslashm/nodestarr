# Disk health and pools

With Proxmox connected (see [Proxmox VE](proxmox.md)), Nodestarr follows
every physical disk of your nodes and the storage pools they form:

- each disk's **SMART data**, kept as history and judged into a verdict
  (healthy, watch, needs attention, failing) with plain-language reasons;
- **ZFS pools** and **LVM thin pools**, plus **mdadm** and **SnapRAID**
  arrays with host access: their state, how many more disk failures they
  survive, capacity and growth, and scrub, resilver and rebuild progress;
- **SMART self-tests**, started by hand or on schedules (with host access).

It's on Proxmox > **Disks**, with a page per disk.

## How Nodestarr reads the disks

Proxmox's API can read SMART data and the state of ZFS and LVM, so all of
that works with the API token alone. Two things it can't do: run SMART
self-tests, and read a disk that is spun down without waking it up.

For those, turn on **host access**: Nodestarr connects to each node over
SSH and runs a small helper script there (below). With host access:

- self-tests and their schedules work;
- SMART data comes complete from `smartctl` (the drive's self-test log,
  lifetime temperatures, error recovery settings, link speed …);
- **disks that are asleep stay asleep**: Nodestarr skips them and keeps
  their last reading (Settings > Proxmox > "Wake disks that are spun down"
  changes that);
- mdadm and SnapRAID arrays show up, ZFS scrubs can be started, paused and
  stopped, and a disk's activity light can be made to blink so you can find
  it in the case.

Through the API, every read wakes a sleeping disk. Nodestarr notices disks
that spin down between readings (their start count grows each time it reads
them) and reads those only about once a day; their page says so. To leave
sleeping disks alone completely (a SnapRAID array, a backup disk), set up
host access.

Nodestarr reads SMART data every 60 minutes by default (Settings >
Proxmox > Disk health) and pool status every 5 minutes; **Read now** on the
Disks page reads everything right away.

## Setting up host access

Settings > Proxmox > Host access:

1. Turn on **Connect to the nodes over SSH**, and save.
2. Under *Setting up host access*, click **Create key**. Nodestarr keeps
   an ed25519 key in its data folder (`ssh/id_ed25519`).
3. Copy the **commands** and run them as root in each node's shell (in the
   Proxmox interface: the node > Shell). They install the helper as
   `/usr/local/sbin/nodestarr-host` and add this line to
   `/root/.ssh/authorized_keys`:

   ```
   restrict,command="/usr/local/sbin/nodestarr-host" ssh-ed25519 AAAA… nodestarr@…
   ```

   In a cluster, `authorized_keys` is shared by all nodes but the helper
   lives on each node, so run the commands on every node (running them twice
   doesn't add the key twice).
4. Click **Test** next to each node. The first successful test pins the
   node's SSH host key; later connections refuse a different one. Compare the
   fingerprint shown with `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub`
   on the node.

Nodestarr reaches a node at its cluster address (from Proxmox), or for a
single node at the API's host. Enter an address per node to use another one
(and save before testing). The SSH user is `root` and the port 22; both can
be changed.

Nodestarr needs the OpenSSH client (`ssh` and `ssh-keygen`) where it runs.
The container image includes it.

### What the key can do

The `command=` in `authorized_keys` makes SSH run the helper whatever is
asked for, and `restrict` turns off forwarding and terminals: the key can't
open a shell. The helper accepts only these commands, checks every argument
against a strict pattern (disk paths, pool names, numbers) and never passes
anything to a shell:

| Command | What it does |
|---|---|
| `info` | Versions of smartctl, ZFS, mdadm and SnapRAID on the node. |
| `smart [--wake] DEVICE…` | `smartctl -j -x` for each disk; skips sleeping ones unless `--wake`. |
| `selftest DEVICE short\|long\|conveyance\|abort` | Starts or aborts a SMART self-test. |
| `selftest-status DEVICE…` | A running test's progress and the self-test log. |
| `lsblk` | Block devices, `/dev/disk/by-*` links and mounted file systems, to map pool members to disks. |
| `mdstat` | `/proc/mdstat`. |
| `snapraid [status]` | `/etc/snapraid.conf` and when the content files last changed; with `status`, also `snapraid status`, unless another snapraid command is running. Nodestarr asks for the status at most every 6 hours. |
| `zpool-scrub POOL start\|pause\|stop` | Starts, pauses or stops a scrub. |
| `identify DEVICE SECONDS` | Reads from a disk for up to 5 minutes so its activity light blinks. |

Self-tests and scrubs started through the helper are logged to the node's
syslog (`nodestarr-host`).

To remove host access, delete the line from `authorized_keys` and
`/usr/local/sbin/nodestarr-host`. **Create a new key** replaces Nodestarr's
key (run the setup commands again, and remove the old line). After
reinstalling a node, **Forget key** next to it, then test again.

## The Disks page

- **Summary**: how many disks need attention, pool health, the hottest
  disk, running self-tests and the next scheduled one.
- **Pools**: each pool's capacity (with its growth over the last 30 days
  and when it'll be full at that pace), how many more disk failures it
  survives, its vdevs or groups drawn as drive bays (colored by each disk's
  health, with temperatures), scrub or rebuild progress, and what needs
  attention. Pool members are matched to disks through their by-id names,
  partition UUIDs and mount points; a member that's gone links to the disk
  it was.
- **Disks**: every disk with its verdict, pool, temperature (with the last
  day as a sparkline), age, bad sectors, wear and last self-test. Click one
  for its page.
- **Self-tests** and **schedules**, disks that are no longer connected,
  and recent changes.

A disk's page has its findings, history charts (temperature, bad sectors,
cable errors, wear or head parks, data written, time asleep) from a week to
everything kept, its self-tests (Nodestarr's and the drive's own log), its
identity, the SMART attributes with what they mean, and its event history.
Use **Where it is** to note its bay, purchase date and warranty end; the bay
shows everywhere the disk does. **Copy summary as text** gives a summary for
a forum post, and **Download smartctl output** the raw JSON.

Disks are identified by model and serial number, so their history follows
them when `/dev/sdX` names change, when they move to another controller, or
to another node of the cluster. Disks with an unusable serial number (some
USB bridges report all zeros) fall back to their WWN, or to node and device.

## What the verdicts mean

Nodestarr judges each disk from its SMART data, how the numbers changed
over the last 30 days, and its pools:

| Finding | Level |
|---|---|
| The drive's own SMART health check fails | failing |
| An attribute is at or below its failure threshold now | failing (pre-fail attributes) or needs attention |
| The last self-test failed (with the first bad block) | failing |
| NVMe critical warning, spare capacity below its minimum | failing or needs attention |
| Unreadable (pending) or uncorrectable sectors | needs attention |
| Reallocated sectors: growing, or more than 100 | needs attention |
| Reallocated sectors: a few, stable | watch |
| New cable (interface CRC) errors | watch (an old, stable count is only noted) |
| New media errors (NVMe), uncorrected errors (SAS) | needs attention (old, stable media errors: watch) |
| 90% or more of the rated write endurance used | needs attention (75%: watch) |
| Hotter than the limit | needs attention (above the drive's own maximum: failing) |
| Read errors the disk couldn't correct, SMART turned off | watch |
| Helium leaking, spin-up retries, 300,000 head parks or more | watch (helium at 25 or less: failing) |

It also notes things worth knowing that don't change the verdict: a
self-test that didn't finish, a disk never self-tested, a SATA link slower
than the disk supports (often the cable or port), a drive-managed SMR disk
in a ZFS pool or mdadm array, error recovery control off in an mdadm array,
heads parking 500 times a day or more, and five years or more of power-on
time.

The temperature limit is the **Disk temperature** alert's threshold (50 °C
by default) for hard disks; SSDs and NVMe drives use their own reported
limit, or 20 °C more. The alert only counts current readings: not the last
one of a disk that's asleep or read only daily.

## Self-tests

- **Short** (about 2 minutes): checks the electronics, the heads and a
  sample of the surface.
- **Long**: reads the whole surface, hours on large hard disks. The drive's
  own estimate is shown before you start.
- **Conveyance** (a few minutes, ATA disks only): checks for damage from
  shipping.

The disk stays usable during a test. Start one from a disk's menu or page;
it waits in a queue when:

- the disk's pool is resilvering, rebuilding or scrubbing (until it
  finishes; after a day the test is skipped);
- **Self-tests at once** (Settings > Proxmox) is reached on that node;
- host access to the node isn't working (after an hour the test gives up).

Nodestarr follows the test and reads the result from the drive's own log:
passed, failed (with the first failing block), aborted, or didn't finish.
Self-tests stop when a disk is reset, powered off or put to sleep: if tests
keep getting interrupted, check the disk's spin-down timer (`hdparm -S`).
Tests keep running in the drive while Nodestarr restarts.

**Schedules** run a test on all disks, on hard disks, SSDs or NVMe drives,
or on chosen disks, optionally per node, at times you set (cron syntax with
presets). **Add a weekly short and a monthly long test** sets up the usual
rhythm. Schedules are part of Settings > Proxmox and are exported with the
settings.

## Pools

- **ZFS** (through the API): state, layout (mirror, RAID-Z1/2/3, dRAID,
  stripes), log, cache, spare, special and dedup vdevs, read, write and
  checksum errors per disk, permanent data errors, the last scrub and a
  running scrub, resilver or RAID-Z expansion. With host access: **Scrub
  now**, pause, resume and stop.
- **mdadm** (host access): level, members, faulty and spare disks, rebuilds
  and checks with their progress.
- **SnapRAID** (host access): data and parity disks (split parity
  included), last sync, how old the oldest scrubbed data is, errors, and
  whether a parity disk is too small for the fullest data disk.
- **LVM thin pools** (through the API): data and metadata use. A thin pool
  whose metadata fills up can be damaged, so Nodestarr warns from 80%
  metadata use.

"Survives N disk failures" is per vdev for ZFS (losing any vdev loses the
pool), and for SnapRAID only covers data that was synced.

## Alerts and notifications

New alert rules (Settings > Monitoring and alerts):

| Rule | Fires when | Severity |
|---|---|---|
| Disk health | a disk needs attention or is failing (not counting temperature) | error |
| Disk temperature | a hard disk is hotter than 50 °C for 10 minutes (SSDs and NVMe drives: their own limit, or 20 °C more) | warning |
| Pool health | a pool is degraded or faulted, has data errors, a scrub or SnapRAID found errors, a SnapRAID parity disk is smaller than the fullest data disk, or a thin pool's metadata is over 80% full | error |
| Pool scrub age | a ZFS pool or SnapRAID array wasn't scrubbed for 35 days | info |

Self-tests that fail or don't finish notify you through the event **Disk
self-test** (`disk.test`); passed ones too if you turn on "Also notify about
passed self-tests". The activity log records new and missing disks, health
changes, pool state changes, finished scrubs and self-tests.

## Home Assistant and Prometheus

With [Home Assistant](home-assistant.md) connected, each disk is a device
with a temperature sensor and a problem binary sensor (with the verdict,
reason and key SMART numbers as attributes), and each pool has a problem
binary sensor and a space used sensor.

[Prometheus metrics](monitoring.md#prometheus) include each disk's verdict,
SMART result, temperature, bad sectors, cable errors, power-on hours, wear
and sleep, and each pool's health, size, use, remaining failure tolerance
and last scrub.

## Limits

- Disks behind a hardware RAID controller usually show only the
  controller's virtual disk, without SMART data.
- Some USB enclosures don't pass SMART through; those disks show "No SMART
  data".
- NVMe self-tests need smartmontools 7.3 or newer on the node (Proxmox VE 8
  and later have it) and a drive that supports them.
- Only the nodes' physical disks are covered, not the virtual disks of
  guests. Disks on a controller passed through to a VM (a TrueNAS VM, say)
  belong to that VM, so the node doesn't see them.
- History is kept for 730 days by default (Settings > Proxmox).

## Troubleshooting host access

| Message | What to do |
|---|---|
| The node didn't accept Nodestarr's key | The `authorized_keys` line is missing or belongs to an older key: run the setup commands again. |
| The helper isn't installed on this node | Run the setup commands on that node (in a cluster: on every node). |
| The node's SSH host key doesn't match | The node was reinstalled, or something else answers at its address. If you know why, click **Forget key** and test again. |
| The helper on … is older than this Nodestarr | Run the setup commands again to install the current helper. |
| smartctl isn't installed | `apt install smartmontools` on the node. |
| Can't reach …:22 | Check the address (enter one per node if needed) and that Nodestarr can reach the node's SSH port. |
| The OpenSSH client (ssh) isn't installed where Nodestarr runs | The container image has it; without the image, install it (`apt install openssh-client`). |
| The SSH client can't run as Nodestarr's user | The container runs with a user ID that has no entry in its `/etc/passwd`, which SSH requires. Run it as root (the default). |

For development there's a stand-in for the helper and a fake disk fleet:
see [dev/README.md](../dev/README.md).
