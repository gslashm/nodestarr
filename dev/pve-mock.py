"""A small Proxmox VE API mock for developing Nodestarr's Proxmox support.

Serves https://127.0.0.1:8006/api2/json with a self-signed certificate and
the response shapes of the real API (PVE 9): version, nodes, node status,
cluster resources, guest power actions, snapshots, vzdump backups, storage
content, tasks with logs, apt updates, disks and RRD data. Actions create
tasks that finish after a couple of seconds and change the guest's state.

Usage: python3 dev/pve-mock.py [port]

Then, in Settings > Proxmox: address https://127.0.0.1:8006, token ID
nodestarr@pve!nodestarr, secret 11111111-2222-3333-4444-555555555555, and
"Test connection" to pin the certificate. Needs Python 3.10+ and openssl.
"""
import json
import math
import os
import random
import re
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import fakedisks

TOKEN = "PVEAPIToken=nodestarr@pve!nodestarr=11111111-2222-3333-4444-555555555555"
NODE = "pve"
# A second cluster member, only listed in the cluster resources (so the
# connection test still fills in NODE on its own).
NODE2 = "pve2"
GIB = 1024**3
START = int(time.time()) - 6 * 86400
LOCK = threading.Lock()

# "use" is what a running guest reports: CPU (a range, as a fraction of its
# cores), memory in GiB, and network bytes in and out.
guests = {
    100: {"type": "lxc", "name": "docker", "status": "running", "maxcpu": 4, "maxmem": 8 * GIB, "maxdisk": 256 * GIB, "template": 0, "tags": "docker;media",
          "use": {"cpu": (0.02, 0.15), "mem": 7.3, "netin": 123_456_789, "netout": 23_456_789}},
    101: {"type": "qemu", "name": "windows-test", "status": "stopped", "maxcpu": 4, "maxmem": 8 * GIB, "maxdisk": 128 * GIB, "template": 0, "tags": ""},
    102: {"type": "lxc", "name": "debian-template", "status": "stopped", "maxcpu": 1, "maxmem": 512 * 1024**2, "maxdisk": 8 * GIB, "template": 1, "tags": ""},
    103: {"type": "lxc", "name": "pihole", "status": "running", "maxcpu": 1, "maxmem": 512 * 1024**2, "maxdisk": 8 * GIB, "template": 0, "tags": "dns",
          "use": {"cpu": (0.01, 0.03), "mem": 0.25, "netin": 41_234_567, "netout": 38_765_432}},
    104: {"type": "lxc", "name": "jellyfin", "status": "running", "maxcpu": 2, "maxmem": 4 * GIB, "maxdisk": 32 * GIB, "template": 0, "tags": "media",
          "use": {"cpu": (0.05, 0.25), "mem": 2.4, "netin": 9_876_543, "netout": 456_789_012}},
    200: {"type": "lxc", "name": "backup-server", "node": NODE2, "status": "running", "maxcpu": 2, "maxmem": 4 * GIB, "maxdisk": 64 * GIB, "template": 0, "tags": "",
          "use": {"cpu": (0.01, 0.05), "mem": 1.6, "netin": 3_456_789_012, "netout": 12_345_678}},
}
snapshots = {100: [{"name": "before_sonarr_v5", "description": "Before moving Sonarr to v5", "snaptime": START + 3600}], 101: [], 102: [], 103: [], 104: [], 200: []}
backups = [
    {"volid": "backups:backup/vzdump-lxc-100-2026_10_13-03_00_01.tar.zst", "content": "backup", "ctime": START + 4 * 86400, "format": "tar.zst", "size": 9_812_345_678, "vmid": 100, "subtype": "lxc", "notes": "docker"},
    {"volid": "backups:backup/vzdump-lxc-100-2026_10_14-03_00_02.tar.zst", "content": "backup", "ctime": START + 5 * 86400, "format": "tar.zst", "size": 9_901_234_567, "vmid": 100, "subtype": "lxc", "notes": "docker", "protected": 1},
]
tasks = []  # newest last
counter = [0x1000]


def upid(kind, vid):
    counter[0] += 1
    now = int(time.time())
    return f"UPID:{NODE}:{counter[0]:08X}:{counter[0] * 7:08X}:{now:08X}:{kind}:{vid}:nodestarr@pve!nodestarr:"


def start_task(kind, vid, lines, effect=None, fail=None, seconds=2):
    u = upid(kind, vid)
    task = {"upid": u, "node": NODE, "pid": counter[0], "pstart": counter[0] * 7, "starttime": int(time.time()), "type": kind, "id": str(vid), "user": "nodestarr@pve!nodestarr", "log": [], "running": True}
    tasks.append(task)

    def run():
        for line in lines:
            time.sleep(seconds / max(1, len(lines)))
            with LOCK:
                task["log"].append(line)
        with LOCK:
            if effect and not fail:
                effect()
            task["running"] = False
            task["endtime"] = int(time.time())
            task["status"] = fail or "OK"
            task["log"].append(f"TASK ERROR: {fail}" if fail else "TASK OK")

    threading.Thread(target=run, daemon=True).start()
    return u


def wave(t, period, lo, hi):
    return lo + (hi - lo) * (0.5 + 0.5 * math.sin(t / period))


def rrd(kind, vid, timeframe):
    step = {"hour": 60, "day": 1800, "week": 10800, "month": 43200, "year": 604800}.get(timeframe, 60)
    now = int(time.time()) // step * step
    out = []
    for i in range(70):
        t = now - (69 - i) * step
        if kind == "node":
            out.append({"time": t, "cpu": wave(t, 900, 0.03, 0.22), "maxcpu": 8, "iowait": wave(t, 600, 0.0, 0.02), "loadavg": wave(t, 700, 0.2, 1.4),
                        "memtotal": 16 * GIB, "memused": wave(t, 2000, 9.5, 12.5) * GIB, "swaptotal": 8 * GIB, "swapused": 0.4 * GIB,
                        "roottotal": 94 * GIB, "rootused": 31 * GIB, "netin": wave(t, 500, 2e4, 4e6), "netout": wave(t, 450, 1e4, 9e5)})
        else:
            g = guests[vid]
            running = g["status"] == "running"
            out.append({"time": t, "cpu": wave(t, 800, 0.02, 0.3) if running else 0, "maxcpu": g["maxcpu"], "mem": wave(t, 1500, 6, 9) * GIB if running else 0,
                        "maxmem": g["maxmem"], "disk": 0, "maxdisk": g["maxdisk"], "netin": wave(t, 500, 1e4, 3e6) if running else 0,
                        "netout": wave(t, 450, 1e4, 7e5) if running else 0, "diskread": wave(t, 300, 0, 2e6) if running else 0, "diskwrite": wave(t, 350, 0, 1e6) if running else 0})
    return out


def guest_resource(vid, g):
    running = g["status"] == "running"
    up = int(time.time()) - START if running else 0
    use = g.get("use", {"cpu": (0.01, 0.2), "mem": 1, "netin": 0, "netout": 0})
    return {"id": f"{g['type']}/{vid}", "type": g["type"], "vmid": vid, "name": g["name"], "node": g.get("node", NODE), "status": g["status"],
            "cpu": random.uniform(*use["cpu"]) if running else 0, "maxcpu": g["maxcpu"], "mem": int(use["mem"] * GIB) if running else 0, "maxmem": g["maxmem"],
            "disk": int(0.4 * g["maxdisk"]), "maxdisk": g["maxdisk"], "netin": use["netin"] if running else 0, "netout": use["netout"] if running else 0,
            "diskread": 3_456_789_012 if running else 0, "diskwrite": 2_345_678_901 if running else 0, "uptime": up, "template": g["template"], "tags": g["tags"]}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        sys.stderr.write("%s %s\n" % (self.command, self.path))

    def send(self, code, payload):
        body = json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json;charset=UTF-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def ok(self, data):
        self.send(200, {"data": data})

    def fail(self, code, message):
        self.send(code, {"data": None, "message": message})

    def form(self):
        n = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(n).decode() if n else ""
        return dict(urllib.parse.parse_qsl(raw))

    def route(self, method):
        url = urllib.parse.urlparse(self.path)
        path = urllib.parse.unquote(url.path)
        query = dict(urllib.parse.parse_qsl(url.query))
        if not path.startswith("/api2/json"):
            return self.fail(404, "not found")
        path = path[len("/api2/json"):]
        if path == "/version" and method == "GET":
            return self.ok({"version": "9.0.10", "release": "9.0", "repoid": "deadbeef"})
        if self.headers.get("Authorization") != TOKEN:
            return self.fail(401, "authentication failure")
        body = self.form() if method == "POST" else {}
        with LOCK:
            return self.dispatch(method, path, query, body)

    def dispatch(self, method, path, query, body):
        if path == "/nodes" and method == "GET":
            return self.ok([{"node": NODE, "status": "online", "id": f"node/{NODE}", "type": "node", "cpu": 0.06, "maxcpu": 8, "mem": 11 * GIB, "maxmem": 16 * GIB,
                             "disk": 31 * GIB, "maxdisk": 94 * GIB, "uptime": int(time.time()) - START, "level": "", "ssl_fingerprint": "1E:7E"}])
        if path == f"/nodes/{NODE}/status" and method == "GET":
            return self.ok({"uptime": int(time.time()) - START, "loadavg": ["0.42", "0.37", "0.31"], "cpu": 0.18, "wait": 0.002, "idle": 0,
                            "memory": {"total": 16 * GIB, "used": 11 * GIB, "free": 5 * GIB, "available": 5 * GIB}, "swap": {"total": 8 * GIB, "used": int(0.4 * GIB), "free": int(7.6 * GIB)},
                            "rootfs": {"total": 94 * GIB, "used": 31 * GIB, "avail": 63 * GIB, "free": 63 * GIB},
                            "cpuinfo": {"model": "Intel(R) Core(TM) i7-7700K CPU @ 4.20GHz", "cores": 4, "sockets": 1, "cpus": 8, "mhz": "4200.000", "hvm": "1", "user_hz": 100},
                            "kversion": "Linux 6.14.11-2-pve #1 SMP PREEMPT_DYNAMIC PMX 6.14.11-2 (2025-09-12T09:46Z)", "pveversion": "pve-manager/9.0.10/deadbeef",
                            "ksm": {"shared": 0}, "boot-info": {"mode": "efi", "secureboot": 0},
                            "current-kernel": {"sysname": "Linux", "release": "6.14.11-2-pve", "machine": "x86_64", "version": "#1 SMP PREEMPT_DYNAMIC"}})
        if path == "/cluster/resources" and method == "GET":
            res = [{"id": f"node/{NODE}", "type": "node", "node": NODE, "status": "online", "cpu": 0.18, "maxcpu": 8, "mem": 11 * GIB, "maxmem": 16 * GIB,
                    "disk": 31 * GIB, "maxdisk": 94 * GIB, "uptime": int(time.time()) - START, "level": "", "cgroup-mode": 2},
                   {"id": f"node/{NODE2}", "type": "node", "node": NODE2, "status": "online", "cpu": 0.04, "maxcpu": 4, "mem": 3 * GIB, "maxmem": 8 * GIB,
                    "disk": 12 * GIB, "maxdisk": 60 * GIB, "uptime": int(time.time()) - START, "level": "", "cgroup-mode": 2}]
            res += [guest_resource(v, g) for v, g in guests.items()]
            res += [
                {"id": f"storage/{NODE}/local", "type": "storage", "storage": "local", "node": NODE, "status": "available", "plugintype": "dir", "content": "iso,vztmpl,backup", "shared": 0, "disk": 31 * GIB, "maxdisk": 94 * GIB},
                {"id": f"storage/{NODE}/local-lvm", "type": "storage", "storage": "local-lvm", "node": NODE, "status": "available", "plugintype": "lvmthin", "content": "rootdir,images", "shared": 0, "disk": 151 * GIB, "maxdisk": 348 * GIB},
                {"id": f"storage/{NODE}/backups", "type": "storage", "storage": "backups", "node": NODE, "status": "available", "plugintype": "dir", "content": "backup", "shared": 0, "disk": int(13.9 * 1024 * GIB), "maxdisk": int(15.5 * 1024 * GIB)},
                {"id": f"sdn/{NODE}/localnetwork", "type": "sdn", "sdn": "localnetwork", "node": NODE, "status": "ok"},
            ]
            return self.ok(res)
        m = re.fullmatch(rf"/nodes/{NODE}/rrddata", path)
        if m and method == "GET":
            return self.ok(rrd("node", None, query.get("timeframe", "hour")))
        m = re.fullmatch(r"/nodes/([\w-]+)/(lxc|qemu)/(\d+)/(.*)", path)
        if m:
            node, kind, vid, rest = m.group(1), m.group(2), int(m.group(3)), m.group(4)
            g = guests.get(vid)
            if not g or g["type"] != kind or g.get("node", NODE) != node:
                return self.fail(500, f"Configuration file 'nodes/{node}/{kind}/{vid}.conf' does not exist")
            return self.guest(method, kind, vid, g, rest, query, body)
        m = re.fullmatch(rf"/nodes/{NODE}/storage/([\w-]+)/content", path)
        if m and method == "GET":
            store = m.group(1)
            return self.ok([b for b in backups if b["volid"].startswith(store + ":")] if query.get("content") in (None, "backup") else [])
        if path == f"/nodes/{NODE}/vzdump" and method == "POST":
            vid = int(body.get("vmid", "0"))
            store = body.get("storage", "local")
            notes = body.get("notes-template", "")
            mode = body.get("mode", "snapshot")
            g = guests.get(vid)
            if not g:
                return self.fail(400, "parameter verification failed: vmid")
            stamp = time.strftime("%Y_%m_%d-%H_%M_%S")

            def done():
                backups.append({"volid": f"{store}:backup/vzdump-{g['type']}-{vid}-{stamp}.tar.zst", "content": "backup", "ctime": int(time.time()), "format": "tar.zst",
                                "size": 9_950_000_000, "vmid": vid, "subtype": g["type"], "notes": notes.replace("{{guestname}}", g["name"]),
                                **({"protected": 1} if body.get("protected") == "1" else {})})
            lines = [f"INFO: starting new backup job: vzdump {vid} --storage {store} --mode {mode} --compress zstd", f"INFO: Starting Backup of VM {vid} ({g['type']})",
                     f"INFO: Backup started at {time.strftime('%Y-%m-%d %H:%M:%S')}", "INFO: status = running", f"INFO: CT Name: {g['name']}",
                     "INFO: creating vzdump archive", "INFO: Total bytes written: 9950000000 (9.3GiB, 410MiB/s)", f"INFO: Finished Backup of VM {vid} (00:00:24)"]
            return self.ok(start_task("vzdump", vid, lines, done, seconds=3))
        if path == f"/nodes/{NODE}/tasks" and method == "GET":
            limit = int(query.get("limit", "50"))
            out = []
            for t in reversed(tasks[-limit:]):
                item = {k: v for k, v in t.items() if k not in ("log", "running")}
                out.append(item)
            return self.ok(out)
        m = re.fullmatch(rf"/nodes/{NODE}/tasks/(.+)/(log|status)", path)
        if m and method == "GET":
            t = next((t for t in tasks if t["upid"] == m.group(1)), None)
            if not t:
                return self.fail(500, "no such task")
            if m.group(2) == "status":
                st = {"upid": t["upid"], "node": NODE, "type": t["type"], "id": t["id"], "user": t["user"], "starttime": t["starttime"], "pid": t["pid"], "pstart": t["pstart"]}
                st.update({"status": "running"} if t["running"] else {"status": "stopped", "exitstatus": t["status"]})
                return self.ok(st)
            start = int(query.get("start", "0"))
            limit = int(query.get("limit", "500"))
            lines = [{"n": i + 1, "t": line} for i, line in enumerate(t["log"])][start:start + limit]
            return self.ok(lines)
        if path == f"/nodes/{NODE}/apt/update":
            if method == "GET":
                return self.ok([
                    {"Package": "pve-manager", "Title": "Proxmox Virtual Environment Management Tools", "OldVersion": "9.0.9", "Version": "9.0.10", "Priority": "important", "Origin": "Proxmox", "Section": "admin", "Arch": "amd64", "Description": "..."},
                    {"Package": "proxmox-kernel-6.14", "Title": "Latest Proxmox Kernel Image", "OldVersion": "6.14.11-1", "Version": "6.14.11-2", "Priority": "optional", "Origin": "Proxmox", "Section": "admin", "Arch": "all", "Description": "..."},
                    {"Package": "openssl", "Title": "Secure Sockets Layer toolkit", "OldVersion": "3.5.1-1", "Version": "3.5.1-1+deb13u1", "Priority": "optional", "Origin": "Debian", "Section": "utils", "Arch": "amd64", "Description": "..."},
                ])
            return self.ok(start_task("aptupdate", "", ["starting apt-get update", "Hit:1 http://deb.debian.org/debian trixie InRelease", "Reading package lists...", "Done"], seconds=1))
        if path == "/cluster/status" and method == "GET":
            return self.ok([
                {"type": "cluster", "id": "cluster", "name": "homelab", "nodes": 2, "quorate": 1, "version": 4},
                {"type": "node", "id": f"node/{NODE}", "name": NODE, "ip": "127.0.0.1", "online": 1, "local": 1, "nodeid": 1, "level": ""},
                {"type": "node", "id": f"node/{NODE2}", "name": NODE2, "ip": "127.0.0.2", "online": 1, "local": 0, "nodeid": 2, "level": ""},
            ])
        m = re.fullmatch(r"/nodes/([\w-]+)/(time|disks/list|disks/smart|disks/zfs|disks/lvm|disks/lvmthin)", path)
        if m and method == "GET":
            node, what = m.group(1), m.group(2)
            if node not in fakedisks.NODES:
                return self.fail(500, f"no such node '{node}'")
            if what == "time":
                now = int(time.time())
                return self.ok({"time": now, "localtime": now + time.localtime().tm_gmtoff, "timezone": time.tzname[0]})
            if what == "disks/list":
                disks = fakedisks.pve_disk_list(node)
                if query.get("include-partitions") not in ("1", "true"):
                    disks = [d for d in disks if "parent" not in d]
                return self.ok(disks)
            if what == "disks/smart":
                data = fakedisks.pve_smart(node, query.get("disk", ""))
                return self.ok(data) if data else self.fail(500, f"disk '{query.get('disk')}' not found")
            if what == "disks/zfs":
                return self.ok(fakedisks.pve_zfs(node)[0])
            if what == "disks/lvm":
                return self.ok(fakedisks.pve_lvm(node)[0])
            return self.ok(fakedisks.pve_lvm(node)[1])
        m = re.fullmatch(r"/nodes/([\w-]+)/disks/zfs/([\w.:-]+)", path)
        if m and method == "GET":
            detail = fakedisks.pve_zfs(m.group(1))[1].get(m.group(2))
            return self.ok(detail) if detail else self.fail(500, f"pool '{m.group(2)}' not found")
        return self.fail(501, f"Method '{method} {path}' not implemented")

    def guest(self, method, kind, vid, g, rest, query, body):
        if rest == "rrddata" and method == "GET":
            return self.ok(rrd("guest", vid, query.get("timeframe", "hour")))
        m = re.fullmatch(r"status/(start|stop|shutdown|reboot|suspend|resume)", rest)
        if m and method == "POST":
            action = m.group(1)
            if g["template"]:
                return self.fail(500, "you can't start a template")
            new = {"start": "running", "stop": "stopped", "shutdown": "stopped", "reboot": "running", "suspend": "paused", "resume": "running"}[action]

            def effect():
                g["status"] = new
            label = {"start": "vzstart" if kind == "lxc" else "qmstart", "stop": "vzstop" if kind == "lxc" else "qmstop",
                     "shutdown": "vzshutdown" if kind == "lxc" else "qmshutdown", "reboot": "vzreboot" if kind == "lxc" else "qmreboot",
                     "suspend": "vzsuspend" if kind == "lxc" else "qmsuspend", "resume": "vzresume" if kind == "lxc" else "qmresume"}[action]
            return self.ok(start_task(label, vid, [f"{action} {kind} {vid}"], effect, seconds=1))
        if rest == "snapshot" and method == "GET":
            items = [dict(s) for s in snapshots[vid]]
            for i, s in enumerate(items):
                if i:
                    s["parent"] = items[i - 1]["name"]
            items.append({"name": "current", "description": "You are here!", "running": 1 if g["status"] == "running" else 0, **({"parent": items[-1]["name"]} if items else {})})
            return self.ok(items)
        if rest == "snapshot" and method == "POST":
            name = body.get("snapname", "")
            if not re.fullmatch(r"[A-Za-z][A-Za-z0-9_\-]{1,39}", name):
                return self.fail(400, "parameter verification failed: snapname")
            if any(s["name"] == name for s in snapshots[vid]):
                return self.fail(500, f"snapshot name '{name}' already used")

            def effect():
                snapshots[vid].append({"name": name, "description": body.get("description", ""), "snaptime": int(time.time()), **({"vmstate": 1} if body.get("vmstate") == "1" else {})})
            return self.ok(start_task("vzsnapshot" if kind == "lxc" else "qmsnapshot", vid, [f"create snapshot '{name}'"], effect))
        m = re.fullmatch(r"snapshot/([^/]+)(/rollback)?", rest)
        if m and method in ("DELETE", "POST"):
            name = m.group(1)
            if not any(s["name"] == name for s in snapshots[vid]):
                return self.fail(500, f"snapshot '{name}' does not exist")
            if method == "DELETE":
                def effect():
                    snapshots[vid][:] = [s for s in snapshots[vid] if s["name"] != name]
                return self.ok(start_task("vzdelsnapshot" if kind == "lxc" else "qmdelsnapshot", vid, [f"delete snapshot '{name}'"], effect))
            if m.group(2):
                return self.ok(start_task("vzrollback" if kind == "lxc" else "qmrollback", vid, [f"rollback to snapshot '{name}'"]))
        return self.fail(501, f"Method '{method} {rest}' not implemented")

    def do_GET(self):
        self.route("GET")

    def do_POST(self):
        self.route("POST")

    def do_DELETE(self):
        self.route("DELETE")


def self_signed(directory):
    """Creates a throwaway self-signed certificate, like a fresh Proxmox node has."""
    cert, key = os.path.join(directory, "cert.pem"), os.path.join(directory, "key.pem")
    subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", cert, "-days", "30",
         "-subj", "/CN=pve.lan", "-addext", "subjectAltName=DNS:pve.lan,IP:127.0.0.1"],
        check=True, capture_output=True)
    return cert, key


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8006
    httpd = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    with tempfile.TemporaryDirectory() as tmp:
        ctx.load_cert_chain(*self_signed(tmp))
    httpd.socket = ctx.wrap_socket(httpd.socket, server_side=True)
    print(f"Proxmox VE mock on https://127.0.0.1:{port}", flush=True)
    httpd.serve_forever()
