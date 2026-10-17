"""A stand-in for Nodestarr's host helper (src/proxmox/disks/helper.sh) on
the fake nodes of pve-mock.py, so host access works without SSH and real
disks.

Start Nodestarr with
    NODESTARR_HOST_HELPER="python3 dev/host-helper.py"
and it runs this instead of connecting to the nodes over SSH (the node name
comes in NODESTARR_NODE). Turn on host access in Settings > Proxmox; the key
and connection test work as usual. Self-tests run in accelerated time: a
short test takes a minute, a long one five. The long test of tank's disk
with pending sectors (sdc) fails at a bad block.

It can be run by hand too: python3 dev/host-helper.py smart /dev/sda
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fakedisks  # noqa: E402

NODE = os.environ.get("NODESTARR_NODE", "pve")


def fail(code, msg):
    print(f"nodestarr-host: {msg}", file=sys.stderr)
    sys.exit(code)


def device(arg):
    d = fakedisks.find(NODE, arg)
    if not arg.startswith("/dev/"):
        fail(65, f"not a disk: {arg}")
    if d is None:
        fail(66, f"no such device: {arg}")
    return d


def main(argv):
    if not argv:
        fail(64, "usage: nodestarr-host COMMAND [ARGUMENTS]")
    cmd, args = argv[0], argv[1:]
    if cmd == "info":
        print(json.dumps({"helper": 1, "hostname": NODE, "time": int(fakedisks.time.time()), "smartctl": "7.4", "zfs": "2.3.4-pve1",
                          "mdadm": NODE == "pve", "md_arrays": NODE == "pve", "snapraid": "12.4" if NODE == "pve" else None,
                          "snapraid_conf": NODE == "pve", "lsblk": True}))
    elif cmd in ("smart", "selftest-status"):
        wake = bool(args) and args[0] == "--wake"
        devs = args[1:] if wake else args
        if not devs:
            fail(64, f"usage: {cmd} DEVICE...")
        out = []
        for dev in devs:
            d = device(dev)
            if cmd == "smart" and d["asleep"] and not wake:
                out.append({"device": dev, "exit": 15, "data": None})
            else:
                if cmd == "smart":
                    fakedisks.wake(NODE, d)
                out.append({"device": dev, "exit": 0, "data": fakedisks.smartctl_json(NODE, d)})
        print(json.dumps(out))
    elif cmd == "selftest":
        if len(args) != 2:
            fail(64, "usage: selftest DEVICE short|long|conveyance|abort")
        d = device(args[0])
        kind = args[1]
        if kind == "abort":
            fakedisks.abort_test(NODE, d)
            print(json.dumps({"smartctl": {"exit_status": 0}}))
        elif kind in fakedisks.TEST_SECONDS:
            if kind == "conveyance" and d["kind"] != "hdd":
                print(json.dumps({"smartctl": {"exit_status": 4, "messages": [{"string": "Conveyance Self-test functions not supported", "severity": "error"}]}}))
                sys.exit(4)
            if not fakedisks.start_test(NODE, d, kind):
                print(json.dumps({"smartctl": {"exit_status": 4, "messages": [{"string": "Can't start self-test without aborting current test (60% remaining),", "severity": "error"}]}}))
                sys.exit(4)
            print(json.dumps({"smartctl": {"exit_status": 0}, "device": {"name": args[0]}}))
        else:
            fail(65, f"unknown self-test type: {kind}")
    elif cmd == "lsblk":
        print(json.dumps(fakedisks.lsblk(NODE)))
    elif cmd == "mdstat":
        sys.stdout.write(fakedisks.mdstat(NODE))
    elif cmd == "snapraid":
        sys.stdout.write(fakedisks.snapraid(NODE, args[:1] == ["status"]))
    elif cmd == "zpool-scrub":
        if len(args) != 2 or args[1] not in ("start", "pause", "stop"):
            fail(64, "usage: zpool-scrub POOL start|pause|stop")
        print(json.dumps({"pool": args[0], "ok": True}))
    elif cmd == "identify":
        if len(args) != 2:
            fail(64, "usage: identify DEVICE SECONDS")
        device(args[0])
        print(json.dumps({"device": args[0], "seconds": int(args[1])}))
    else:
        fail(64, f"unknown command: {cmd} (Nodestarr may be newer than this helper; reinstall it)")


if __name__ == "__main__":
    main(sys.argv[1:])
