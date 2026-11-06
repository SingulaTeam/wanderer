#!/usr/bin/env bash
# Wanderer's leak test: the daemon, Xray and the Network Guard, for real, with
# no root and nothing touched outside the test.
#
#   XRAY=/path/to/xray scripts/leak-test/run.sh
#
# Everything runs in a user namespace of its own, so "root" here is root only
# over the test's own network and mount namespaces:
#
#   client namespace (the user's machine)        internet namespace
#   ┌───────────────────────────────────┐        ┌───────────────────────────────┐
#   │ wandererd ─ xray ─ wanderer0 (TUN)│  veth  │ node: xray, REALITY on :443   │
#   │ veth0 203.0.113.2                 ├────────┤ veth1 203.0.113.1             │
#   │ lan0  10.9.0.2 (the local network)│        │ web   93.184.215.14 :8080 :8443│
#   └───────────────────────────────────┘        └───────────────────────────────┘
#
# The web server answers with the address a request came from: the node's
# when it went through the tunnel, the client's when it did not. Leaks are
# counted by capturing what the client sends on veth0 and lan0, apart from
# the tunnel itself — never by the address the web server sees.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"

if [[ -z "${WANDERER_LEAK_TEST_INNER:-}" ]]; then
	: "${XRAY:?set XRAY to an Xray binary}"
	(cd "$ROOT" && cargo build -q -p wanderer-daemon) || exit 1
	export WANDERER_LEAK_TEST_INNER=1 XRAY="$(realpath "$XRAY")"
	# `unshare --fork` ignores SIGTERM and SIGINT, and every process below
	# would inherit that; give them back their defaults.
	exec unshare --user --map-root-user --net --mount --fork env --default-signal "$0" "$@"
fi

WORK="$(mktemp -d)"
DAEMON="$ROOT/target/debug/wandererd"
SOCKET="$WORK/daemon.sock"
NODE="203.0.113.1"
# Any public address will do: the real one is out of reach from here, and the
# node's Xray refuses to connect to reserved ranges such as the TEST-NETs.
WEB="93.184.215.14"
CP="$WEB:8443" # stands in for a Control Plane the app must reach while holding
UUID="7316e3b4-53eb-41da-90d9-23bbde8e1cc6"
FAILED=0

cleanup() {
	# Not `jobs -p | xargs kill`: a pipeline runs in a subshell with no jobs.
	kill $(jobs -p) 2>/dev/null
	for _ in $(seq 30); do [[ -z "$(jobs -rp)" ]] && break; sleep 0.1; done
	if [[ -n "$(jobs -rp)" ]]; then
		echo "still running after SIGTERM:" >&2
		jobs -rl >&2
		kill -KILL $(jobs -rp) 2>/dev/null
	fi
	wait 2>/dev/null
	[[ -n "${LEAK_TEST_LOGS:-}" ]] && cp -r "$WORK/." "$LEAK_TEST_LOGS/"
	rm -rf "$WORK"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# The host's systemd-resolved cannot see this namespace's interfaces; hide it,
# as on a machine without it.
mount -t tmpfs none /run/systemd/resolve 2>/dev/null
# And /sys shows the host's interfaces until it is mounted again from here.
mount -t sysfs none /sys || exit 1

# ── The network ─────────────────────────────────────────────────────────────
unshare --net sleep infinity &
NET=$!
sleep 0.2
net() { nsenter --target "$NET" --net "$@"; }

ip link set lo up
ip link add veth0 type veth peer name veth1 netns "$NET"
ip addr add 203.0.113.2/24 dev veth0
ip -6 addr add fd00:113::2/64 dev veth0 nodad
ip link set veth0 up
ip route add default via "$NODE"
ip -6 route add default via fd00:113::1
ip link add lan0 type dummy
ip addr add 10.9.0.2/24 dev lan0
ip link set lan0 up

net ip link set lo up
net ip addr add "$NODE/24" dev veth1
net ip -6 addr add fd00:113::1/64 dev veth1 nodad
net ip link set veth1 up
net ip link add web type dummy
net ip addr add "$WEB/32" dev web
net ip addr add 1.1.1.1/32 dev web # the resolver the daemon asks
net ip link set web up

openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 1 \
	-subj /CN=www.example.com -keyout "$WORK/key.pem" -out "$WORK/cert.pem" 2>/dev/null
# Started directly, not through `net`, so $! is the process itself.
nsenter --target "$NET" --net python3 -I "$HERE/internet.py" "$WEB" "$WORK/cert.pem" "$WORK/key.pem" 1.1.1.1 \
	>>"$WORK/internet.log" 2>&1 &
# Ready before the first probe, or that probe measures the test, not Wanderer.
for _ in $(seq 50); do curl -s -m 1 -o /dev/null "http://$WEB:8080/" && break; sleep 0.1; done

start_node() {
	nsenter --target "$NET" --net "$XRAY" run -c "$HERE/node.json" >>"$WORK/node.log" 2>&1 &
	NODE_PID=$!
	sleep 0.5
}
stop_node() { kill "$NODE_PID" 2>/dev/null; wait "$NODE_PID" 2>/dev/null; }
start_node

# Each `ctl` below is a short connection of its own, so the daemon's wait
# for an app that went away is off until the scenario that tests it.
start_daemon() {
	"$DAEMON" --socket "$SOCKET" --xray "$XRAY" --app-grace "$1" >>"$WORK/daemon.log" 2>&1 &
	DAEMON_PID=$!
	for _ in $(seq 50); do [[ -S "$SOCKET" ]] && break; sleep 0.1; done
}
start_daemon 0

# ── Talking to the daemon ───────────────────────────────────────────────────
ctl() {
	python3 -I -c '
import json, socket, sys
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode() + b"\n")
reply = json.loads(s.makefile().readline())
if "addresses" in reply:
    print(reply["reply"], " ".join(reply["addresses"]))
else:
    print(reply["reply"], (reply.get("status") or reply)["state"], reply.get("message") or "")
' "$SOCKET" "$1"
}
guard() { printf '{"allowed":["%s"],"allow_lan":%s}' "$CP" "$1"; }
connect() {
	local transport=$1 lan=${2:-true}
	ctl "$(printf '{"request":"connect","plan":{"node":"%s:443","uuid":"%s","reality":{"public_key":"Vfv6lG2gjLQSbkLJdcH72Eo7zW2LrNRhAyrvU_YPJGQ","short_id":"a1b2c3d4","server_name":"www.example.com","fingerprint":"chrome"},"transport":%s},"guard":%s}' \
		"$NODE" "$UUID" "$transport" "$(guard "$lan")")"
}
VISION='{"kind":"vision"}'
XHTTP='{"kind":"xhttp","path":"/api/v1/session/heartbeat"}'

# ── Probes ──────────────────────────────────────────────────────────────────
capture_start() {
	python3 -I "$HERE/capture.py" veth0 "$NODE:443" "$CP" >"$WORK/out.count" 2>>"$WORK/capture.log" &
	DUMP_OUT=$!
	python3 -I "$HERE/capture.py" lan0 >"$WORK/lan.count" 2>>"$WORK/capture.log" &
	DUMP_LAN=$!
	for _ in $(seq 50); do
		grep -q ready "$WORK/out.count" && grep -q ready "$WORK/lan.count" && break
		sleep 0.1
	done
}
capture_stop() {
	sleep 0.5
	kill -TERM "$DUMP_OUT" "$DUMP_LAN" 2>/dev/null
	wait "$DUMP_OUT" "$DUMP_LAN" 2>/dev/null
	count() { sed -n "s/.*\b$2=\([0-9]*\).*/\1/p" "$WORK/$1.count"; }
	LEAKED=$(count out leaked)
	LAN_DNS=$(count lan lan_dns)
	LAN=$(count lan lan)
}

probe() {
	capture_start
	local web cp
	web=$(curl -s -m 3 "http://$WEB:8080/")
	cp=$(curl -sk --http1.1 -m 3 -o /dev/null -w '%{http_code}' "https://$CP/")
	dig +time=2 +tries=1 +short "r$RANDOM.example.com" @1.1.1.1 >/dev/null 2>&1
	# Not A or AAAA: Xray forwards it, through the tunnel, or nothing answers.
	dig +time=3 +tries=1 TXT "t$RANDOM.example.com" @1.1.1.1 2>/dev/null | grep -q "status: NOERROR" &&
		TXT=answered || TXT=none
	dig +time=2 +tries=1 +short example.com @10.9.0.1 >/dev/null 2>&1
	curl -s -m 2 -o /dev/null "http://10.9.0.1:8080/"
	curl -s -m 2 -o /dev/null "http://[fd00:113::1]:8080/"
	capture_stop
	case "$web" in
	"$WEB") WEB_VIA=tunnel ;;
	203.0.113.2) WEB_VIA=direct ;;
	*) WEB_VIA=none ;;
	esac
	[[ "$cp" == 000 ]] && CP_REACHED=no || CP_REACHED=yes
	printf '    web: %-6s  cp: %-3s  txt: %-8s  leaked: %-3s  lan dns: %-3s  lan: %s\n' \
		"$WEB_VIA" "$CP_REACHED" "$TXT" "$LEAKED" "$LAN_DNS" "$LAN"
}

expect() {
	local what=$1 got=$2 want=$3
	if [[ "$got" != "$want" ]]; then
		echo "    FAIL: $what is $got, expected $want"
		FAILED=1
	fi
}
sealed() {
	expect "traffic outside the tunnel" "$LEAKED" 0
	expect "DNS to the local network" "$LAN_DNS" 0
}

# ── Scenarios ───────────────────────────────────────────────────────────────
echo "Wanderer leak test — Xray $("$XRAY" version | head -1 | awk '{print $2}')"

echo "before connecting (the test must see traffic when there is no guard):"
probe
expect "web" "$WEB_VIA" direct
[[ "$LEAKED" -gt 0 ]] || { echo "    FAIL: the capture sees nothing"; FAILED=1; }

echo "connect, Vision:"
echo "    daemon: $(connect "$VISION")"
probe
expect "web" "$WEB_VIA" tunnel
expect "a TXT query" "$TXT" answered
sealed

echo "Xray crashes:"
pkill -KILL -f -- "-c stdin:"
sleep 0.5
echo "    daemon: $(ctl '{"request":"status"}')"
probe
expect "web" "$WEB_VIA" none
expect "the Control Plane" "$CP_REACHED" yes
sealed

echo "reconnect, xHTTP:"
echo "    daemon: $(connect "$XHTTP")"
probe
expect "web" "$WEB_VIA" tunnel
sealed

echo "the node goes away (Xray keeps running):"
stop_node
probe
expect "web" "$WEB_VIA" none
sealed
start_node

echo "the node is back:"
for attempt in 1 2 3 4 5; do
	probe
	sealed
	[[ "$WEB_VIA" == tunnel ]] && break
	sleep 2
done
echo "    through again after $attempt probes"
expect "web" "$WEB_VIA" tunnel

echo "hold, with the local network closed:"
echo "    daemon: $(ctl "{\"request\":\"hold\",\"guard\":$(guard false)}")"
probe
expect "web" "$WEB_VIA" none
expect "the Control Plane" "$CP_REACHED" yes
expect "traffic to the local network" "$LAN" 0
sealed
echo "    the daemon resolves a node's name meanwhile: $(ctl '{"request":"resolve","host":"node.example"}')"
expect "the daemon's resolution" "$(ctl '{"request":"resolve","host":"node.example"}')" "resolved $WEB"

echo "disconnect:"
echo "    daemon: $(ctl '{"request":"disconnect"}')"
probe
expect "web" "$WEB_VIA" direct

echo "the app goes away while connected (a 3 s wait for it):"
kill "$DAEMON_PID"
wait "$DAEMON_PID" 2>/dev/null
start_daemon 3
echo "    daemon: $(connect "$VISION")"
sleep 5
state=$(ctl '{"request":"status"}')
echo "    daemon: $state"
expect "the daemon" "$state" "ok off "
probe
expect "web" "$WEB_VIA" direct

echo
if [[ $FAILED == 0 ]]; then echo "all sealed"; else
	echo "FAILED — daemon log:"
	tail -20 "$WORK/daemon.log"
fi
exit "$FAILED"
