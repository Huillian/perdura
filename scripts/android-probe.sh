#!/usr/bin/env bash
# Physical probe. Runs perdura on this computer and on Android devices
# (through adb) that share the same network, then reports ICMPv6
# reachability, who discovered whom, and which pairs exchanged a message
# over the encrypted channel.
# It reports every result and does not stop at the first failure.
#
# Build first:
#   cargo build -p perdura-cli
#   cargo ndk -t arm64-v8a build --release -p perdura-cli
#
# Usage: scripts/android-probe.sh <adb-serial> [<adb-serial> ...]
# Any adb serial format is accepted.
set -uo pipefail

HOST_BIN="$(pwd)/target/debug/perdura"
ANDROID_BIN="$(pwd)/target/aarch64-linux-android/release/perdura"
REMOTE_BIN=/data/local/tmp/perdura
REMOTE_DATA=/data/local/tmp/perdura-data
DISCOVERY_SECONDS=20
LISTEN_SECONDS=15
PORT=29170

if [ "$#" -lt 1 ]; then
    echo "usage: $0 <adb-serial> [<adb-serial> ...]" >&2
    exit 2
fi
for file in "$HOST_BIN" "$ANDROID_BIN"; do
    if [ ! -x "$file" ]; then
        echo "missing $file" >&2
        exit 2
    fi
done

OUT="$(mktemp -d)"

# Node 0 is this computer. Its interface is the one that routes to the
# first device.
FIRST_IP="$(adb -s "$1" shell "ip -4 -o addr show dev wlan0" | awk '{print $4}' | cut -d/ -f1 | head -n 1 | tr -d '\r')"
HOST_DEV="$(ip -o route get "$FIRST_IP" | sed -n 's/.* dev \([^ ]*\).*/\1/p')"
NAMES=("host")
SERIALS=("")
INDEXES=("$(ip -o link show dev "$HOST_DEV" | cut -d: -f1)")
ADDRESSES=("$(ip -6 -o addr show dev "$HOST_DEV" scope link | awk '{print $4}' | cut -d/ -f1 | head -n 1)")

count=0
for serial in "$@"; do
    count=$((count + 1))
    adb -s "$serial" push "$ANDROID_BIN" "$REMOTE_BIN" >/dev/null
    adb -s "$serial" shell chmod 755 "$REMOTE_BIN"
    NAMES+=("device$count")
    SERIALS+=("$serial")
    INDEXES+=("$(adb -s "$serial" shell "ip -6 -o addr show dev wlan0 scope link" | cut -d: -f1 | head -n 1 | tr -d '\r')")
    ADDRESSES+=("$(adb -s "$serial" shell "ip -6 -o addr show dev wlan0 scope link" | awk '{print $4}' | cut -d/ -f1 | head -n 1 | tr -d '\r')")
    model="$(adb -s "$serial" shell getprop ro.product.model | tr -d '\r')"
    release="$(adb -s "$serial" shell getprop ro.build.version.release | tr -d '\r')"
    version="$(adb -s "$serial" shell "$REMOTE_BIN" | tr -d '\r')"
    echo "device$count: $model, Android $release, $version"
done

# run <node> <perdura arguments...>
run() {
    local node="$1"
    shift
    if [ "$node" -eq 0 ]; then
        "$HOST_BIN" "$@"
    else
        adb -s "${SERIALS[$node]}" shell "$REMOTE_BIN $(printf '%q ' "$@")"
    fi
}

data_dir() {
    if [ "$1" -eq 0 ]; then
        echo "$OUT/host-data"
    else
        echo "$REMOTE_DATA"
    fi
}

# ping_from <node> <target-node>
ping_from() {
    local node="$1" target="$2"
    if [ "$node" -eq 0 ]; then
        ping -6 -c 2 -W 2 "${ADDRESSES[$target]}%$HOST_DEV" >/dev/null 2>&1
    else
        adb -s "${SERIALS[$node]}" shell "ping6 -c 2 -W 2 -I wlan0 ${ADDRESSES[$target]}" >/dev/null 2>&1
    fi
}

echo "== nodes =="
echo "host interface: $HOST_DEV"
for i in "${!NAMES[@]}"; do
    echo "${NAMES[$i]}: ifindex=${INDEXES[$i]:-MISSING} address=${ADDRESSES[$i]:-MISSING}"
done

echo "== reachability (ICMPv6) =="
for i in "${!NAMES[@]}"; do
    for j in "${!NAMES[@]}"; do
        if [ "$i" -eq "$j" ]; then
            continue
        fi
        result="FAILED"
        if ping_from "$i" "$j"; then
            result="ok"
        fi
        echo "ping: ${NAMES[$i]} -> ${NAMES[$j]}: $result"
    done
done

echo "== discovery (${DISCOVERY_SECONDS}s) =="
for i in "${!NAMES[@]}"; do
    run "$i" discover --ifindex "${INDEXES[$i]}" --duration "$DISCOVERY_SECONDS" \
        >"$OUT/discover-${NAMES[$i]}.log" 2>&1 &
done
wait

IDS=()
for i in "${!NAMES[@]}"; do
    IDS+=("$(awk '/^node /{print $2}' "$OUT/discover-${NAMES[$i]}.log" | tr -d '\r')")
done
for i in "${!NAMES[@]}"; do
    for j in "${!NAMES[@]}"; do
        if [ "$i" -eq "$j" ]; then
            continue
        fi
        result="NO"
        if [ -n "${IDS[$j]}" ] && grep -q "^found ${IDS[$j]} " "$OUT/discover-${NAMES[$i]}.log"; then
            result="yes"
        fi
        echo "discovery: ${NAMES[$i]} saw ${NAMES[$j]}: $result"
    done
    echo "  ${NAMES[$i]} $(grep -E '^(stats|error|usage)' "$OUT/discover-${NAMES[$i]}.log" | tr -d '\r' | tail -n 1)"
done

echo "== channel =="
for listener in "${!NAMES[@]}"; do
    for sender in "${!NAMES[@]}"; do
        if [ "$listener" -eq "$sender" ]; then
            continue
        fi
        pair="${NAMES[$sender]}-to-${NAMES[$listener]}"
        target="[${ADDRESSES[$listener]}%${INDEXES[$sender]}]:$PORT"
        run "$listener" listen --data-dir "$(data_dir "$listener")" --duration "$LISTEN_SECONDS" \
            >"$OUT/listen-$pair.log" 2>&1 &
        sleep 2
        run "$sender" send --data-dir "$(data_dir "$sender")" --to "$target" --message probe \
            >"$OUT/send-$pair.log" 2>&1
        if grep -q '^reply probe' "$OUT/send-$pair.log"; then
            result="ok"
        else
            result="FAILED: $(tail -n 1 "$OUT/send-$pair.log" | tr -d '\r')"
        fi
        echo "channel: ${NAMES[$sender]} -> ${NAMES[$listener]}: $result"
        wait
    done
done

echo "logs kept in $OUT"
