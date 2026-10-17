"""The disks, pools and arrays of the fake Proxmox nodes, shared by
pve-mock.py (what the Proxmox API shows) and host-helper.py (what the
Nodestarr host helper shows), so both tell the same story.

Node "pve" has:
- rpool: a ZFS mirror of two NVMe drives (the system),
- tank: RAID-Z2 over six 4 TB WD Reds, one with pending sectors, a scrub
  running, a mirrored log and a cache device,
- backup: a ZFS mirror of two 8 TB Seagates, one of them pulled (DEGRADED),
- a SnapRAID array: two data disks (one asleep) and a parity disk, pooled
  with mergerfs,
- md0: an mdadm RAID 1 of two SATA SSDs, rebuilding,
- a USB backup disk with a bogus serial number, and the LVM thin pool
  pve/data on the first NVMe drive.
Node "pve2" has a single NVMe drive with rpool.

Self-tests run in accelerated time (a short test takes a minute) and their
state lives in a file, because every helper call is a new process. So do the
start counts of the sleeping disk: every read that wakes it (through the
Proxmox API, or the helper's `smart --wake`) adds one, as on a real disk.
"""
import json
import math
import os
import tempfile
import time
import zlib

GIB = 1024**3
START = int(time.time()) - 6 * 86400
STATE_FILE = os.path.join(tempfile.gettempdir(), "nodestarr-host-helper-mock.json")
TEST_SECONDS = {"short": 60, "long": 300, "conveyance": 90}


def disk(dev, model, serial, size, kind, **kw):
    d = {
        "dev": dev, "model": model, "serial": serial, "size": size, "kind": kind,
        "rpm": 0 if kind != "hdd" else kw.pop("rpm", 5400), "wwn": kw.pop("wwn", None),
        "firmware": kw.pop("firmware", "01.01A01"), "family": kw.pop("family", None),
        "poh": kw.pop("poh", 20_000), "cycles": kw.pop("cycles", 80), "temp": kw.pop("temp", 34),
        "realloc": kw.pop("realloc", 0), "pending": kw.pop("pending", 0), "offline": kw.pop("offline", 0),
        "crc": kw.pop("crc", 0), "load_cycles": kw.pop("load_cycles", 5_000), "wear": kw.pop("wear", None),
        "written": kw.pop("written", None), "asleep": kw.pop("asleep", False), "usb": kw.pop("usb", False),
        "fail_long": kw.pop("fail_long", False), "smart_failed": kw.pop("smart_failed", False),
        "used": kw.pop("used", None), "parts": kw.pop("parts", []), "link": kw.pop("link", ("6.0 Gb/s", "6.0 Gb/s")),
    }
    assert not kw, kw
    return d


def wd_red(dev, n, **kw):
    return disk(dev, "WDC WD40EFRX-68N32N0", f"WD-WCC7K000000{n}", 4_000_787_030_016, "hdd", family="Western Digital Red",
                firmware="82.00A82", wwn=f"0x50014ee2b3c4d5e{n}", poh=41_000 + n * 50, used="ZFS", parts=[1, 9], **kw)


NODES = {
    "pve": [
        disk("nvme0n1", "Samsung SSD 970 EVO Plus 500GB", "S4EVNX0N123456", 500_107_862_016, "nvme", firmware="2B2QEXM7",
             wwn="eui.0025385b71b07e2f", poh=12_345, wear=7, written=48_000_000_000_000, temp=41, used="ZFS", parts=[1, 2, 3, 4]),
        disk("nvme1n1", "Samsung SSD 970 EVO Plus 500GB", "S4EVNX0N654321", 500_107_862_016, "nvme", firmware="2B2QEXM7",
             wwn="eui.0025385b71b07e30", poh=12_345, wear=6, written=46_000_000_000_000, temp=39, used="ZFS", parts=[1, 2, 3]),
        wd_red("sda", 1, temp=36),
        wd_red("sdb", 2, temp=37),
        wd_red("sdc", 3, temp=38, realloc=8, pending=2, fail_long=True),
        wd_red("sdd", 4, temp=37, crc=3),
        wd_red("sde", 5, temp=39),
        wd_red("sdf", 6, temp=35),
        disk("sdg", "WDC WD80EFZZ-68BTXN0", "WD-CA0AB123", 8_001_563_222_016, "hdd", family="Western Digital Red Plus",
             wwn="0x5000cca0beef0001", poh=9_000, temp=33, used="ext4", parts=[1]),
        disk("sdh", "WDC WD80EFZZ-68BTXN0", "WD-CA0AB456", 8_001_563_222_016, "hdd", family="Western Digital Red Plus",
             wwn="0x5000cca0beef0002", poh=9_100, temp=31, used="ext4", parts=[1], asleep=True),
        disk("sdi", "TOSHIBA HDWG11A", "X0R0A0QFFAXG", 10_000_831_348_736, "hdd", family="Toshiba N300/MN NAS HDD",
             wwn="0x5000039a8bcd0001", rpm=7200, poh=15_500, temp=40, used="ext4", parts=[1]),
        disk("sdj", "Samsung SSD 860 EVO 250GB", "S3YJNB0K100001A", 250_059_350_016, "ssd", wwn="0x5002538e40000001",
             poh=30_000, wear=31, written=95_000_000_000_000, temp=30, used="mdraid", parts=[1]),
        disk("sdk", "Samsung SSD 860 EVO 250GB", "S3YJNB0K100002B", 250_059_350_016, "ssd", wwn="0x5002538e40000002",
             poh=30_100, wear=93, written=410_000_000_000_000, temp=31, used="mdraid", parts=[1], link=("6.0 Gb/s", "3.0 Gb/s")),
        disk("sdl", "ST2000DM008-2FR102", "000000000000", 2_000_398_934_016, "hdd", family="Seagate BarraCuda 3.5 (SMR)",
             rpm=7200, poh=3_000, temp=45, used="ext4", parts=[1], usb=True),
        disk("sdm", "ST8000VE001-3CC101", "WRQ032XP", 8_001_563_222_016, "hdd", family="Seagate SkyHawk", rpm=7200,
             wwn="0x5000c500a1b2c301", poh=17_000, temp=42, used="ZFS", parts=[1, 9], load_cycles=380_000),
    ],
    "pve2": [
        disk("nvme0n1", "CT1000P3SSD8", "2316E6A0B1C2", 1_000_204_886_016, "nvme", firmware="P9CR30A", wwn="eui.6479a7a1b2c3d4e5",
             poh=4_000, wear=2, written=12_000_000_000_000, temp=44, used="ZFS", parts=[1, 2, 3]),
    ],
}

# The second backup disk was pulled: ZFS remembers it by GUID.
PULLED_BACKUP_DISK = "/dev/disk/by-id/ata-ST8000VE001-3CC101_WRQ032YQ-part1"


def by_id(d):
    bus = "nvme" if d["kind"] == "nvme" else ("usb" if d["usb"] else "ata")
    return f"/dev/disk/by-id/{bus}-{d['model'].replace(' ', '_')}_{d['serial']}"


def part(d, n):
    return f"/dev/{d['dev']}{'p' if d['dev'].startswith('nvme') else ''}{n}"


def find(node, dev):
    dev = dev.replace("/dev/", "")
    return next((d for d in NODES.get(node, []) if d["dev"] == dev or by_id(d).endswith("/" + dev.rsplit("/", 1)[-1])), None)


def hours(d):
    return d["poh"] + (int(time.time()) - START) // 3600


def temperature(d):
    t = time.time()
    return round(d["temp"] + 2 * math.sin(t / 900 + d["poh"]))


# ---------------------------------------------------------------------------
# Self-test state (shared between helper calls through a file)
# ---------------------------------------------------------------------------

def load_state():
    try:
        with open(STATE_FILE) as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def save_state(state):
    tmp = STATE_FILE + ".tmp"
    with open(tmp, "w") as f:
        json.dump(state, f)
    os.replace(tmp, STATE_FILE)


def wake(node, d):
    """Reading a sleeping disk spins it up (and it goes back to sleep later)."""
    if d["asleep"]:
        state = load_state()
        key = f"wakes:{node}:{d['dev']}"
        state[key] = state.get(key, 0) + 1
        save_state(state)


def start_stops(node, d):
    return d["cycles"] * 3 + load_state().get(f"wakes:{node}:{d['dev']}", 0)


def test_state(node, d):
    """(running, log): finishes tests whose time is up."""
    state = load_state()
    key = f"{node}:{d['dev']}"
    entry = state.setdefault(key, {"running": None, "log": [
        {"kind": "short", "status": 0, "hours": hours(d) - 170},
        {"kind": "long", "status": 0, "hours": hours(d) - 700},
    ]})
    run = entry["running"]
    if run and time.time() - run["started"] >= TEST_SECONDS[run["kind"]]:
        failed = run["kind"] == "long" and d["fail_long"]
        entry["log"].insert(0, {"kind": run["kind"], "status": 0x79 if failed else 0, "hours": hours(d), "lba": 123_456_789 if failed else None})
        entry["log"] = entry["log"][:21]
        entry["running"] = None
        save_state(state)
    return entry["running"], entry["log"]


def start_test(node, d, kind):
    state = load_state()
    key = f"{node}:{d['dev']}"
    running, _ = test_state(node, d)
    if running:
        return False
    state = load_state()
    entry = state.setdefault(key, {"running": None, "log": []})
    entry["running"] = {"kind": kind, "started": time.time()}
    save_state(state)
    return True


def abort_test(node, d):
    state = load_state()
    entry = state.get(f"{node}:{d['dev']}")
    if entry and entry.get("running"):
        entry["log"].insert(0, {"kind": entry["running"]["kind"], "status": 0x10, "hours": hours(d)})
        entry["running"] = None
        save_state(state)


# ---------------------------------------------------------------------------
# smartctl -j -x
# ---------------------------------------------------------------------------

def attribute(id_, name, value, worst, thresh, raw, prefail=False):
    return {"id": id_, "name": name, "value": value, "worst": worst, "thresh": thresh, "when_failed": "",
            "flags": {"value": 51 if prefail else 50, "string": "PO--CK " if prefail else "-O--CK ", "prefailure": prefail},
            "raw": {"value": raw if isinstance(raw, int) else 0, "string": str(raw)}}


ATA_TEST_NAMES = {"short": (1, "Short offline"), "long": (2, "Extended offline"), "conveyance": (3, "Conveyance offline")}
ATA_STATUS = {0: "Completed without error", 0x10: "Aborted by host", 0x79: "Completed: read failure"}


def smartctl_json(node, d, mode="x"):
    t = temperature(d)
    out = {
        "json_format_version": [1, 0],
        "smartctl": {"version": [7, 4], "exit_status": 0},
        "device": {"name": f"/dev/{d['dev']}", "type": "nvme" if d["kind"] == "nvme" else "sat",
                   "protocol": "NVMe" if d["kind"] == "nvme" else "ATA"},
        "model_name": d["model"],
        "serial_number": d["serial"],
        "firmware_version": d["firmware"],
        "user_capacity": {"bytes": d["size"]},
        "logical_block_size": 512,
        "smart_support": {"available": True, "enabled": True},
        "smart_status": {"passed": not d["smart_failed"]},
        "power_on_time": {"hours": hours(d)},
        "power_cycle_count": d["cycles"],
        "temperature": {"current": t},
    }
    if d["family"]:
        out["model_family"] = d["family"]
    running, log = test_state(node, d)
    if d["kind"] == "nvme":
        out["nvme_total_capacity"] = d["size"]
        out["temperature"].update({"op_limit_max": 85, "critical_limit_max": 87})
        out["nvme_smart_health_information_log"] = {
            "critical_warning": 0, "temperature": t, "available_spare": 100, "available_spare_threshold": 10,
            "percentage_used": d["wear"], "data_units_read": d["written"] * 2 // 512_000, "data_units_written": d["written"] // 512_000,
            "power_cycles": d["cycles"], "power_on_hours": hours(d), "unsafe_shutdowns": 17, "media_errors": 0, "num_err_log_entries": 0,
        }
        tests = {"current_self_test_operation": {"value": 0, "string": "No self-test in progress"}, "table": [
            {"self_test_code": {"value": 1 if e["kind"] == "short" else 2, "string": "Short" if e["kind"] == "short" else "Extended"},
             "self_test_result": {"value": 0 if e["status"] == 0 else 1, "string": "Completed without error" if e["status"] == 0 else "Aborted by a Device Self-test command"},
             "power_on_hours": e["hours"]} for e in log]}
        if running:
            done = min(99, int((time.time() - running["started"]) / TEST_SECONDS[running["kind"]] * 100))
            tests["current_self_test_operation"] = {"value": 1 if running["kind"] == "short" else 2, "string": "Self-test in progress"}
            tests["current_self_test_completion_percent"] = done
        out["nvme_self_test_log"] = tests
        return out

    out["rotation_rate"] = d["rpm"]
    out["physical_block_size"] = 4096 if d["kind"] == "hdd" else 512
    out["form_factor"] = {"name": "3.5 inches" if d["kind"] == "hdd" else "2.5 inches"}
    out["interface_speed"] = {"max": {"string": d["link"][0]}, "current": {"string": d["link"][1]}}
    out["trim"] = {"supported": d["kind"] == "ssd"}
    out["ata_sct_erc"] = {"read": {"enabled": d["kind"] == "hdd", "deciseconds": 70}, "write": {"enabled": d["kind"] == "hdd", "deciseconds": 70}}
    out["temperature"].update({"lifetime_min": 18, "lifetime_max": max(t + 9, 46), "op_limit_max": 60, "limit_max": 70})
    status = {"value": 0, "string": "completed without error", "passed": True}
    if running:
        remaining = max(1, 9 - int((time.time() - running["started"]) / TEST_SECONDS[running["kind"]] * 10))
        status = {"value": 0xF0 + remaining, "string": f"in progress, {remaining * 10}% remaining", "remaining_percent": remaining * 10}
    out["ata_smart_data"] = {
        "self_test": {"status": status, "polling_minutes": {"short": 2, "extended": 496 if d["kind"] == "hdd" else 30, "conveyance": 5}},
        "capabilities": {"self_tests_supported": True, "conveyance_self_test_supported": d["kind"] == "hdd"},
    }
    attrs = [
        attribute(1, "Raw_Read_Error_Rate", 200, 200, 51, 0, True),
        attribute(5, "Reallocated_Sector_Ct", 200 if not d["realloc"] else 198, 198 if d["realloc"] else 200, 140, d["realloc"], True),
        attribute(9, "Power_On_Hours", 44, 44, 0, hours(d)),
        attribute(12, "Power_Cycle_Count", 100, 100, 0, d["cycles"]),
    ]
    if d["kind"] == "hdd":
        attrs += [
            attribute(4, "Start_Stop_Count", 100, 100, 0, start_stops(node, d)),
            attribute(10, "Spin_Retry_Count", 100, 253, 0, 0),
            attribute(193, "Load_Cycle_Count", 180, 180, 0, d["load_cycles"] + (int(time.time()) - START) // 60),
            attribute(194, "Temperature_Celsius", 116, 104, 0, f"{t} (Min/Max 18/46)"),
            attribute(196, "Reallocated_Event_Count", 200, 200, 0, d["realloc"] // 4),
            attribute(197, "Current_Pending_Sector", 200, 200, 0, d["pending"]),
            attribute(198, "Offline_Uncorrectable", 100, 253, 0, d["offline"]),
            attribute(199, "UDMA_CRC_Error_Count", 200, 200, 0, d["crc"]),
        ]
    else:
        attrs += [
            attribute(177, "Wear_Leveling_Count", 100 - d["wear"], 100 - d["wear"], 0, d["wear"] * 30, True),
            attribute(190, "Airflow_Temperature_Cel", 100 - t, 50, 0, t),
            attribute(199, "UDMA_CRC_Error_Count", 100, 100, 0, d["crc"]),
            attribute(241, "Total_LBAs_Written", 99, 99, 0, d["written"] // 512),
        ]
    attrs.sort(key=lambda a: a["id"])
    out["ata_smart_attributes"] = {"revision": 16, "table": attrs}
    out["ata_smart_error_log"] = {"extended": {"revision": 1, "sectors": 2, "count": 0}}
    table = []
    if running:
        code, label = ATA_TEST_NAMES[running["kind"]]
        table.append({"type": {"value": code, "string": label}, "status": {"value": status["value"], "string": "Self-test routine in progress", "remaining_percent": status.get("remaining_percent")}, "lifetime_hours": hours(d)})
    for e in log:
        code, label = ATA_TEST_NAMES[e["kind"]]
        entry = {"type": {"value": code, "string": label}, "status": {"value": e["status"], "string": ATA_STATUS.get(e["status"], "Unknown"), "passed": e["status"] == 0}, "lifetime_hours": e["hours"]}
        if e.get("lba"):
            entry["lba"] = e["lba"]
        table.append(entry)
    out["ata_smart_self_test_log"] = {"extended": {"revision": 1, "sectors": 1, "table": table, "count": len(table)}}
    return out


# ---------------------------------------------------------------------------
# What the Proxmox API shows
# ---------------------------------------------------------------------------

def pve_disk_list(node):
    out = []
    for d in NODES.get(node, []):
        typ = "usb" if d["usb"] else d["kind"]
        out.append({"devpath": f"/dev/{d['dev']}", "model": d["model"].replace(" ", "_") if d["kind"] == "hdd" and not d["usb"] else d["model"],
                    "serial": d["serial"], "size": d["size"], "type": typ, "health": "UNKNOWN", "wearout": "N/A",
                    "used": d["used"] or "partitions", "gpt": 1, "rpm": d["rpm"], "vendor": "ATA     ", "wwn": d["wwn"] or "unknown",
                    "by_id_link": by_id(d), "osdid": -1, "mounted": 0})
        for p in d["parts"]:
            out.append({"devpath": part(d, p), "parent": f"/dev/{d['dev']}", "type": "partition", "size": d["size"] // max(1, len(d["parts"])), "used": d["used"] or "partition"})
    return out


def pve_smart(node, dev):
    d = find(node, dev)
    if d is None:
        return None
    if d["kind"] == "nvme":
        n = smartctl_json(node, d)["nvme_smart_health_information_log"]
        text = "\n".join([
            "SMART/Health Information (NVMe Log 0x02)",
            f"Critical Warning:                   0x{n['critical_warning']:02x}",
            f"Temperature:                        {n['temperature']} Celsius",
            f"Available Spare:                    {n['available_spare']}%",
            f"Available Spare Threshold:          {n['available_spare_threshold']}%",
            f"Percentage Used:                    {n['percentage_used']}%",
            f"Data Units Read:                    {n['data_units_read']:,} [x TB]",
            f"Data Units Written:                 {n['data_units_written']:,} [x TB]",
            f"Power Cycles:                       {n['power_cycles']}",
            f"Power On Hours:                     {n['power_on_hours']:,}",
            f"Unsafe Shutdowns:                   {n['unsafe_shutdowns']}",
            f"Media and Data Integrity Errors:    {n['media_errors']}",
            f"Error Information Log Entries:      {n['num_err_log_entries']}",
        ])
        return {"health": "PASSED", "type": "text", "text": text + "\n", "wearout": 100 - d["wear"]}
    wake(node, d)
    j = smartctl_json(node, d)
    attrs = [{"id": f"{a['id']:>3}", "name": a["name"], "value": a["value"], "normalized": a["value"], "worst": a["worst"],
              "threshold": a["thresh"], "fail": "-", "flags": a["flags"]["string"].strip(), "raw": a["raw"]["string"]} for a in j["ata_smart_attributes"]["table"]]
    return {"health": "FAILED" if d["smart_failed"] else "PASSED", "type": "ata", "attributes": attrs}


def vdev_disk(d, p=1, state="ONLINE", cksum=0, msg=""):
    return {"name": f"{by_id(d)}-part{p}" if p else by_id(d), "state": state, "read": 0, "write": 0, "cksum": cksum, "msg": msg, "leaf": 1}


def scrub_progress():
    """A scrub of tank that started an hour ago and takes four hours."""
    started = int(time.time()) - 3600
    pct = min(99.9, (time.time() - started) / (4 * 3600) * 100)
    since = time.strftime("%a %b %d %H:%M:%S %Y", time.localtime(started))
    left = int(4 * 3600 * (1 - pct / 100))
    return (f"scrub in progress since {since} 6.21T scanned at 1.71G/s, {6.21 * pct / 100:.2f}T issued at 452M/s, 6.21T total "
            f"0B repaired, {pct:.2f}% done, {left // 3600:02d}:{left % 3600 // 60:02d}:{left % 60:02d} to go")


def pve_zfs(node):
    """(list entries, details by name)."""
    disks = {d["dev"]: d for d in NODES.get(node, [])}
    last_week = time.strftime("%a %b %d %H:%M:%S %Y", time.localtime(time.time() - 9 * 86400))
    if node == "pve2":
        nv = disks["nvme0n1"]
        rpool = {"name": "rpool", "state": "ONLINE", "scan": f"scrub repaired 0B in 00:04:12 with 0 errors on {last_week}", "errors": "No known data errors",
                 "children": [{"name": "rpool", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": "", "children": [vdev_disk(nv, 3)]}]}
        return [{"name": "rpool", "size": 990 * GIB, "alloc": 120 * GIB, "free": 870 * GIB, "frag": 4, "dedup": 1.0, "health": "ONLINE"}], {"rpool": rpool}
    rpool = {"name": "rpool", "state": "ONLINE", "scan": f"scrub repaired 0B in 00:02:51 with 0 errors on {last_week}", "errors": "No known data errors",
             "children": [{"name": "rpool", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": "", "children": [
                 {"name": "mirror-0", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": "", "children": [vdev_disk(disks["nvme0n1"], 3), vdev_disk(disks["nvme1n1"], 3)]}]}]}
    tank = {"name": "tank", "state": "ONLINE", "scan": scrub_progress(), "errors": "No known data errors",
            "children": [
                {"name": "tank", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": "", "children": [
                    {"name": "raidz2-0", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": "", "children": [
                        vdev_disk(disks[x], cksum=2 if x == "sdc" else 0) for x in ("sda", "sdb", "sdc", "sdd", "sde", "sdf")]}]},
                {"name": "logs", "children": [{"name": "mirror-1", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": "", "children": [
                    {"name": "/dev/nvme0n1p1", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": ""},
                    {"name": "/dev/nvme1n1p1", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": ""}]}]},
                {"name": "cache", "children": [{"name": "/dev/nvme0n1p2", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": ""}]},
            ]}
    backup = {"name": "backup", "state": "DEGRADED",
              "status": "One or more devices could not be used because the label is missing or invalid. Sufficient replicas exist for the pool to continue functioning in a degraded state.",
              "action": "Replace the device using 'zpool replace'.", "see": "https://openzfs.github.io/openzfs-docs/msg/ZFS-8000-4J",
              "scan": f"scrub repaired 0B in 05:12:33 with 0 errors on {time.strftime('%a %b %d %H:%M:%S %Y', time.localtime(time.time() - 50 * 86400))}",
              "errors": "No known data errors",
              "children": [{"name": "backup", "state": "DEGRADED", "read": 0, "write": 0, "cksum": 0, "msg": "", "children": [
                  {"name": "mirror-0", "state": "DEGRADED", "read": 0, "write": 0, "cksum": 0, "msg": "", "children": [
                      vdev_disk(disks["sdm"]),
                      {"name": "8829382738492837462", "state": "UNAVAIL", "read": 0, "write": 0, "cksum": 0, "msg": f"was {PULLED_BACKUP_DISK}"}]}]}]}
    entries = [
        {"name": "rpool", "size": 476 * GIB, "alloc": 61 * GIB, "free": 415 * GIB, "frag": 8, "dedup": 1.0, "health": "ONLINE"},
        {"name": "tank", "size": 21_800 * GIB, "alloc": 15_100 * GIB, "free": 6_700 * GIB, "frag": 21, "dedup": 1.0, "health": "ONLINE"},
        {"name": "backup", "size": 7_270 * GIB, "alloc": 6_480 * GIB, "free": 790 * GIB, "frag": 2, "dedup": 1.0, "health": "DEGRADED"},
    ]
    return entries, {"rpool": rpool, "tank": tank, "backup": backup}


def pve_lvm(node):
    if node != "pve":
        return {"leaf": 0, "children": []}, []
    lvm = {"leaf": 0, "children": [{"name": "pve", "size": 400 * GIB, "free": 16 * GIB, "leaf": 0,
                                    "children": [{"name": "/dev/nvme0n1p4", "size": 400 * GIB, "free": 16 * GIB, "leaf": 1}]}]}
    thin = [{"lv": "data", "vg": "pve", "lv_size": 300 * GIB, "used": int(300 * GIB * 0.62), "metadata_size": 3 * GIB, "metadata_used": int(3 * GIB * 0.21)}]
    return lvm, thin


# ---------------------------------------------------------------------------
# What only the host helper sees
# ---------------------------------------------------------------------------

def lsblk(node):
    devs, links = [], {}
    for d in NODES.get(node, []):
        devs.append({"name": d["dev"], "kname": d["dev"], "path": f"/dev/{d['dev']}", "pkname": None, "type": "disk", "size": d["size"],
                     "rota": d["kind"] == "hdd", "tran": "usb" if d["usb"] else ("nvme" if d["kind"] == "nvme" else "sata"), "model": d["model"],
                     "serial": d["serial"], "wwn": d["wwn"]})
        links[by_id(d).rsplit("/", 1)[-1]] = f"/dev/{d['dev']}"
        for p in d["parts"]:
            name = part(d, p).replace("/dev/", "")
            devs.append({"name": name, "kname": name, "path": f"/dev/{name}", "pkname": d["dev"], "type": "part", "size": d["size"] // len(d["parts"]),
                         "partuuid": f"{zlib.crc32(d['serial'].encode()):08x}-0000-4000-8000-{p:012d}", "fstype": "zfs_member" if d["used"] == "ZFS" else "ext4"})
            links[f"{by_id(d).rsplit('/', 1)[-1]}-part{p}"] = f"/dev/{name}"
    mounts = []
    if node == "pve":
        dd = {d["dev"]: d for d in NODES["pve"]}
        for dev, target, used in (("sdg", "/mnt/disk1", 0.71), ("sdh", "/mnt/disk2", 0.64), ("sdi", "/mnt/parity1", 0.72), ("sdl", "/mnt/usb-backup", 0.33)):
            size = dd[dev]["size"] * 98 // 100
            mounts.append({"source": f"/dev/{dev}1", "target": target, "fstype": "ext4", "size": size, "used": int(size * used), "avail": int(size * (1 - used))})
        size = sum(m["size"] for m in mounts[:2])
        mounts.append({"source": "disk1:disk2", "target": "/mnt/storage", "fstype": "fuse.mergerfs", "size": size, "used": int(size * 0.67), "avail": int(size * 0.33)})
        devs.append({"name": "md0", "kname": "md0", "path": "/dev/md0", "pkname": "sdj1", "type": "raid1", "size": dd["sdj"]["size"]})
    return {"lsblk": {"blockdevices": devs}, "links": links, "mounts": {"filesystems": mounts}}


def mdstat(node):
    if node != "pve":
        return "Personalities : \nunused devices: <none>\n"
    pct = (time.time() / 30) % 100
    return (
        "Personalities : [raid1] [linear] [multipath] [raid0] [raid6] [raid5] [raid4] [raid10]\n"
        "md0 : active raid1 sdk1[2] sdj1[0]\n"
        "      244066432 blocks super 1.2 [2/1] [U_]\n"
        f"      [====>................]  recovery = {pct:4.1f}% ({int(244066432 * pct / 100)}/244066432) finish={max(1, (100 - pct) * 0.6):.1f}min speed=98765K/sec\n"
        "      bitmap: 1/2 pages [4KB], 65536KB chunk\n\nunused devices: <none>\n"
    )


SNAPRAID_CONF = """parity /mnt/parity1/snapraid.parity
content /var/snapraid/snapraid.content
content /mnt/disk1/.snapraid.content
content /mnt/disk2/.snapraid.content
data d1 /mnt/disk1/
data d2 /mnt/disk2/
exclude *.unrecoverable
exclude /lost+found/
"""

SNAPRAID_STATUS = """Self test...
Loading state from /var/snapraid/snapraid.content...
Using 1203 MiB of memory for the file-system.
SnapRAID status report:

   Files Fragmented Excess  Wasted  Used    Free  Use Name
            Files  Fragments  GB      GB      GB
   94120     112      803      2.4    5583    2280  71% d1
   88103     140      911      3.1    5011    2850  64% d2
 --------------------------------------------------------------------------
  182223     252     1714      5.5   10594    5130  67%


The oldest block was scrubbed 41 days ago, the median 9, the newest 0.

No sync is in progress.
The 22% of the array is not scrubbed.
No rehash is in progress or needed.
No error detected.
"""


def snapraid(node, with_status):
    if node != "pve":
        return "##none\n"
    synced = int(time.time()) - 2 * 86400 - 3600
    out = "##conf\n" + "\n".join(l for l in SNAPRAID_CONF.splitlines() if l and not l.startswith("#")) + "\n"
    out += "##content\n" + "".join(f"{synced} {p}\n" for p in ("/var/snapraid/snapraid.content", "/mnt/disk1/.snapraid.content"))
    out += "##busy\nno\n"
    if with_status:
        out += "##status\n" + SNAPRAID_STATUS
    return out
