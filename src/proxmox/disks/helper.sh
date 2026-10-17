#!/bin/sh
# Nodestarr host helper, protocol 1.
#
# Runs on a Proxmox VE node as the forced command of the SSH key Nodestarr
# uses, so that key can do nothing but what this script allows:
#
#   info                          versions of the tools below
#   smart [--wake] DEVICE...      SMART data (smartctl -j -x); sleeping disks
#                                 are skipped unless --wake is given
#   selftest DEVICE TYPE          start a short, long or conveyance self-test,
#                                 or "abort" the running one
#   selftest-status DEVICE...     self-test progress and log
#   lsblk                         block devices, /dev/disk links and mounts
#   mdstat                        mdadm arrays (/proc/mdstat)
#   snapraid [status]             SnapRAID config (and `snapraid status`)
#   zpool-scrub POOL ACTION       start, pause or stop a ZFS scrub
#   identify DEVICE SECONDS       read from a disk so its activity light blinks
#
# Arguments are checked against strict patterns and never reach a shell.
# Install it as /usr/local/sbin/nodestarr-host (mode 0755) and authorize the
# key in /root/.ssh/authorized_keys like this:
#
#   restrict,command="/usr/local/sbin/nodestarr-host" ssh-ed25519 AAAA… nodestarr
set -eu
umask 077
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
LC_ALL=C
export PATH LC_ALL

VERSION=1
SNAPRAID_CONF=/etc/snapraid.conf

fail() {
	code=$1
	shift
	printf 'nodestarr-host: %s\n' "$*" >&2
	exit "$code"
}

# Only letters, digits and / _ . : + @ , = - (no spaces, quotes or newlines).
safe() {
	case $1 in
	'' | *[!A-Za-z0-9/_.:+@,=-]*) return 1 ;;
	esac
}

check_dev() {
	safe "$1" || fail 65 "not a disk: $1"
	printf '%s\n' "$1" | grep -Eqx '/dev/(sd[a-z]{1,3}|hd[a-z]|vd[a-z]{1,2}|xvd[a-z]{1,2}|nvme[0-9]{1,3}(n[0-9]{1,3})?|sg[0-9]{1,4}|disk/by-id/[A-Za-z0-9._:+@-]{1,200})' ||
		fail 65 "not a disk: $1"
	[ -b "$1" ] || [ -c "$1" ] || fail 66 "no such device: $1"
}

# A JSON string from text that may contain quotes or backslashes.
jstr() {
	if [ -n "$1" ]; then
		printf '"%s"' "$(printf '%s' "$1" | sed 's/[\\"]/\\&/g')"
	else
		printf 'null'
	fi
}

have() {
	if command -v "$1" >/dev/null 2>&1; then printf 'true'; else printf 'false'; fi
}

note() {
	logger -t nodestarr-host -- "$*" 2>/dev/null || true
}

cmd_info() {
	smartctl_v=$(smartctl -V 2>/dev/null | sed -n '1s/^smartctl \([0-9][0-9.]*\).*/\1/p' || true)
	zfs_v=$(cat /sys/module/zfs/version 2>/dev/null || true)
	snapraid_v=$(snapraid --version 2>/dev/null | sed -n '1s/^snapraid v\{0,1\}\([0-9][0-9.]*\).*/\1/p' || true)
	snapraid_conf=false
	[ -r "$SNAPRAID_CONF" ] && snapraid_conf=true
	md=false
	[ -r /proc/mdstat ] && grep -q '^md' /proc/mdstat && md=true
	printf '{"helper":%s,"hostname":%s,"time":%s,"smartctl":%s,"zfs":%s,"mdadm":%s,"md_arrays":%s,"snapraid":%s,"snapraid_conf":%s,"lsblk":%s}\n' \
		"$VERSION" "$(jstr "$(hostname)")" "$(date +%s)" "$(jstr "$smartctl_v")" "$(jstr "$zfs_v")" \
		"$(have mdadm)" "$md" "$(jstr "$snapraid_v")" "$snapraid_conf" "$(have lsblk)"
}

# One JSON array entry per device: {"device", "exit", "data"}. A sleeping
# disk is skipped with exit status 15 (smartctl -n standby,15).
smart_each() {
	mode=$1
	shift
	printf '['
	sep=''
	for d in "$@"; do
		case $mode in
		wake) out=$(timeout 90 smartctl -j -x "$d" 2>/dev/null) && rc=0 || rc=$? ;;
		sleep) out=$(timeout 90 smartctl -j -x -n standby,15 "$d" 2>/dev/null) && rc=0 || rc=$? ;;
		status) out=$(timeout 60 smartctl -j -c -l selftest "$d" 2>/dev/null) && rc=0 || rc=$? ;;
		esac
		case $out in
		'{'*'}') ;;
		*) out=null ;;
		esac
		printf '%s{"device":"%s","exit":%s,"data":%s}' "$sep" "$d" "$rc" "$out"
		sep=','
	done
	printf ']\n'
}

cmd_smart() {
	mode=sleep
	if [ "${1:-}" = "--wake" ]; then
		mode=wake
		shift
	fi
	[ $# -ge 1 ] || fail 64 "usage: smart [--wake] DEVICE..."
	for d in "$@"; do check_dev "$d"; done
	smart_each "$mode" "$@"
}

cmd_selftest() {
	[ $# -eq 2 ] || fail 64 "usage: selftest DEVICE short|long|conveyance|abort"
	check_dev "$1"
	case $2 in
	short | long | conveyance)
		note "starting a $2 SMART self-test on $1"
		timeout 60 smartctl -j -t "$2" "$1"
		;;
	abort)
		note "aborting the SMART self-test on $1"
		timeout 60 smartctl -j -X "$1"
		;;
	*) fail 65 "unknown self-test type: $2" ;;
	esac
}

cmd_selftest_status() {
	[ $# -ge 1 ] || fail 64 "usage: selftest-status DEVICE..."
	for d in "$@"; do check_dev "$d"; done
	smart_each status "$@"
}

cmd_lsblk() {
	printf '{"lsblk":'
	lsblk -J -l -b -o NAME,KNAME,PATH,PKNAME,TYPE,SIZE,ROTA,TRAN,MODEL,SERIAL,WWN,FSTYPE,LABEL,UUID,PARTUUID,MOUNTPOINT 2>/dev/null || printf '{}'
	printf ',"links":{'
	sep=''
	for l in /dev/disk/by-id/* /dev/disk/by-partuuid/* /dev/disk/by-uuid/* /dev/mapper/*; do
		[ -L "$l" ] || continue
		n=${l##*/}
		safe "$n" || continue
		t=$(readlink -f "$l") || continue
		safe "$t" || continue
		printf '%s"%s":"%s"' "$sep" "$n" "$t"
		sep=','
	done
	printf '},"mounts":'
	findmnt -J -b -l -o SOURCE,TARGET,FSTYPE,SIZE,USED,AVAIL \
		-t ext2,ext3,ext4,xfs,btrfs,vfat,exfat,ntfs,ntfs3,f2fs,fuseblk,fuse.mergerfs 2>/dev/null || printf '{}'
	printf '}\n'
}

cmd_mdstat() {
	if [ -r /proc/mdstat ]; then cat /proc/mdstat; fi
}

snapraid_busy() {
	pgrep -x snapraid >/dev/null 2>&1
}

cmd_snapraid() {
	if [ ! -r "$SNAPRAID_CONF" ]; then
		printf '##none\n'
		return 0
	fi
	printf '##conf\n'
	grep -v '^[[:space:]]*#' "$SNAPRAID_CONF" | grep -v '^[[:space:]]*$' || true
	printf '##content\n'
	grep -E '^[[:space:]]*content[[:space:]]' "$SNAPRAID_CONF" | while read -r _ f; do
		if [ -f "$f" ]; then stat -c '%Y %n' "$f" || true; fi
	done
	printf '##busy\n'
	if snapraid_busy; then echo yes; else echo no; fi
	if [ "${1:-}" = "status" ]; then
		printf '##status\n'
		# Reading the state takes memory and time on big arrays, and a running
		# sync holds the lock: skip it then.
		if snapraid_busy; then
			echo busy
		else
			timeout 900 nice -n 10 snapraid -c "$SNAPRAID_CONF" status 2>&1 || true
		fi
	fi
}

cmd_zpool_scrub() {
	[ $# -eq 2 ] || fail 64 "usage: zpool-scrub POOL start|pause|stop"
	printf '%s\n' "$1" | grep -Eqx '[A-Za-z][A-Za-z0-9_.:-]{0,250}' || fail 65 "not a pool name: $1"
	case $2 in
	start) set -- "$1" "" ;;
	pause) set -- "$1" -p ;;
	stop) set -- "$1" -s ;;
	*) fail 65 "unknown scrub action: $2" ;;
	esac
	note "zpool scrub ${2:-start} on $1"
	if [ -n "$2" ]; then zpool scrub "$2" "$1"; else zpool scrub "$1"; fi
	printf '{"pool":"%s","ok":true}\n' "$1"
}

cmd_identify() {
	[ $# -eq 2 ] || fail 64 "usage: identify DEVICE SECONDS"
	check_dev "$1"
	[ -b "$1" ] || fail 65 "not a block device: $1"
	case $2 in
	[1-9] | [1-9][0-9] | [1-2][0-9][0-9] | 300) ;;
	*) fail 65 "seconds must be between 1 and 300" ;;
	esac
	# Read in the background (and only read), so the command returns at once.
	nohup timeout "$2" dd if="$1" of=/dev/null bs=1M iflag=direct status=none </dev/null >/dev/null 2>&1 &
	printf '{"device":"%s","seconds":%s}\n' "$1" "$2"
}

# A forced command gets the requested command in SSH_ORIGINAL_COMMAND; run
# by hand (to test it), the arguments are used instead.
if [ -n "${SSH_ORIGINAL_COMMAND:-}" ]; then
	case $SSH_ORIGINAL_COMMAND in
	*[!A-Za-z0-9\ /_.:+@,=-]*) fail 65 "unexpected characters in the command" ;;
	esac
	set -f
	# shellcheck disable=SC2086 # Splitting into words is the point.
	set -- $SSH_ORIGINAL_COMMAND
	set +f
fi
[ $# -ge 1 ] || fail 64 "usage: nodestarr-host COMMAND [ARGUMENTS] (see the top of this script)"
cmd=$1
shift
case $cmd in
info) cmd_info ;;
smart) cmd_smart "$@" ;;
selftest) cmd_selftest "$@" ;;
selftest-status) cmd_selftest_status "$@" ;;
lsblk) cmd_lsblk ;;
mdstat) cmd_mdstat ;;
snapraid) cmd_snapraid "$@" ;;
zpool-scrub) cmd_zpool_scrub "$@" ;;
identify) cmd_identify "$@" ;;
*) fail 64 "unknown command: $cmd (Nodestarr may be newer than this helper; reinstall it)" ;;
esac
