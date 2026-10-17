#!/usr/bin/env bash
# End-to-end check of Proxmox disk health without Docker, Proxmox or SSH:
# builds Nodestarr, runs it against pve-mock.py with host-helper.py standing
# in for SSH, reads the disks and pools, and runs a short self-test.
#
#   bash dev/smoke-disks.sh              # self-test on /dev/sdl (no pool)
#   TEST_DEV=/dev/sda bash dev/smoke-disks.sh   # waits: tank is scrubbing
#
# Linux only (Docker's API is a dummy Unix socket that never answers, so
# Nodestarr starts with Docker "unreachable"). Leaves its data and logs in
# $SMOKE_DIR (default ~/nodestarr-smoke).
set -u
REPO=$(cd "$(dirname "$0")/.." && pwd)
W=${SMOKE_DIR:-$HOME/nodestarr-smoke}
PORT=${SMOKE_PORT:-7981}
rm -rf "$W"
mkdir -p "$W/data"
rm -f "${TMPDIR:-/tmp}/nodestarr-host-helper-mock.json"
cd "$REPO"
cargo build --locked -q || exit 1
BIN=${CARGO_TARGET_DIR:-$REPO/target}/debug/nodestarr

python3 -c "import socket,time; s=socket.socket(socket.AF_UNIX); s.bind('$W/docker.sock'); s.listen(64); time.sleep(10**6)" &
DOCK=$!
sleep 1
python3 dev/pve-mock.py >"$W/mock.log" 2>&1 &
MOCK=$!
NODESTARR_DATA_DIR="$W/data" NODESTARR_LISTEN=127.0.0.1:$PORT NODESTARR_LOG="nodestarr=debug,info" \
  NODESTARR_ADMIN_USER=admin NODESTARR_ADMIN_PASSWORD=smoke-test-pw \
  NODESTARR_HOST_HELPER="python3 $REPO/dev/host-helper.py" DOCKER_HOST="unix://$W/docker.sock" \
  "$BIN" >"$W/server.log" 2>&1 &
SRV=$!
trap 'kill $MOCK $SRV $DOCK 2>/dev/null' EXIT

API=http://127.0.0.1:$PORT/api/v1
C=(-s -b "$W/jar" -c "$W/jar" -H 'X-Nodestarr-Request: 1' -H 'Content-Type: application/json')
for _ in $(seq 30); do curl -s "$API/health" >/dev/null && break; sleep 1; done
curl "${C[@]}" -X POST "$API/auth/login" -d '{"username":"admin","password":"smoke-test-pw"}' >/dev/null
curl "${C[@]}" -X PUT "$API/settings/proxmox" -o /dev/null -d '{
  "enabled": true, "url": "https://127.0.0.1:8006", "token_id": "nodestarr@pve!nodestarr",
  "token_secret": "11111111-2222-3333-4444-555555555555", "tls_mode": "insecure", "node": "pve",
  "host_access": {"enabled": true, "user": "root", "port": 22}
}'
echo "== host access test"
curl "${C[@]}" -X POST "$API/proxmox/host-access/test" -d '{"node":"pve"}'
echo
echo "== reading the nodes"
curl "${C[@]}" -X POST "$API/proxmox/disk-health/refresh" -o /dev/null
for _ in $(seq 60); do
  sleep 2
  curl "${C[@]}" "$API/proxmox/disk-health" >"$W/overview.json"
  python3 -c 'import json,sys; s=json.load(open(sys.argv[1]))["sampling"]; sys.exit(0 if not s["running"] and s.get("last_smart") else 1)' "$W/overview.json" 2>/dev/null && break
done
python3 - "$W/overview.json" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
print("sampling:", d["sampling"])
for x in d["disks"]:
    top = x["findings"][0]["title"] if x["findings"] else ""
    roles = [r["pool_name"] + "/" + r["role"] for r in x["roles"]]
    print(f'{x["node"]:5} {x["devpath"]:14} {x["verdict"]:8} src={x["source"]} asleep={x["asleep"]} temp={x["temperature"]} roles={roles} {top}')
for p in d["pools"]:
    print(f'POOL {p["id"]:24} {p["health"]:9} {p["layout"]} tolerance={p["tolerance"]} findings={[f["title"] for f in p["findings"]]}')
EOF

DEV=${TEST_DEV:-/dev/sdl}
echo "== short self-test on $DEV"
ID=$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(next(x["id"] for x in d["disks"] if x["devpath"]==sys.argv[2] and x["node"]=="pve"))' "$W/overview.json" "$DEV")
curl "${C[@]}" -X POST "$API/proxmox/disk-health/disks/$ID/tests" -d '{"kind":"short"}' -o /dev/null
for _ in $(seq 40); do
  sleep 3
  curl "${C[@]}" "$API/proxmox/disk-health/disks/$ID" | python3 -c '
import json, sys
d = json.load(sys.stdin)
t = d["test"]
if t:
    print(t["status"], t.get("progress"), t.get("message") or "")
else:
    t = d["tests"][0]
    print("done:", t["status"], t.get("result"), t.get("message") or "")
' | tee "$W/test.txt" | uniq
  grep -q '^done:' "$W/test.txt" && break
done
echo "== warnings and errors in the server log (Docker ones are expected)"
grep -E " (ERROR|WARN) " "$W/server.log" | head -20
