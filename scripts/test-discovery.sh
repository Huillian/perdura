#!/usr/bin/env bash
# End-to-end discovery test: two nodes in separate network namespaces,
# linked by a veth pair, with no route to anything else.
# Needs: unshare (util-linux), ip (iproute2), unprivileged user namespaces.
set -euo pipefail

SELF="$(readlink -f "$0")"
ROLE="${1:-main}"
DURATION=15

node() {
    local dev="$1" log="$2" index
    index="$(ip -o link show dev "$dev" | cut -d: -f1)"
    "$BIN" discover --ifindex "$index" --duration "$DURATION" >"$log" 2>&1
}

case "$ROLE" in
main)
    BIN="$(pwd)/target/debug/perdura"
    if [ ! -x "$BIN" ]; then
        echo "missing $BIN; run: cargo build -p perdura-cli" >&2
        exit 2
    fi
    TMP="$(mktemp -d)"
    trap 'rm -rf "$TMP"' EXIT
    export BIN TMP

    unshare --user --map-root-user --net bash "$SELF" outer

    A_ID="$(awk '/^node /{print $2}' "$TMP/a.log")"
    B_ID="$(awk '/^node /{print $2}' "$TMP/b.log")"
    if [ -n "$A_ID" ] && [ -n "$B_ID" ] \
        && grep -q "^found $B_ID " "$TMP/a.log" \
        && grep -q "^found $A_ID " "$TMP/b.log"; then
        echo "PASS: both nodes discovered each other"
    else
        echo "FAIL" >&2
        echo "--- node a ---" >&2
        cat "$TMP/a.log" >&2 || true
        echo "--- node b ---" >&2
        cat "$TMP/b.log" >&2 || true
        exit 1
    fi
    ;;
outer)
    ip link add va type veth peer name vb
    unshare --net bash "$SELF" node-b &
    B_JOB=$!
    for _ in $(seq 1 50); do
        [ -s "$TMP/b.pid" ] && break
        sleep 0.1
    done
    ip link set vb netns "$(cat "$TMP/b.pid")"
    ip link set va up
    node va "$TMP/a.log" || true
    wait "$B_JOB" || true
    ;;
node-b)
    echo "$$" >"$TMP/b.pid"
    for _ in $(seq 1 50); do
        ip link show dev vb >/dev/null 2>&1 && break
        sleep 0.1
    done
    ip link set vb up
    node vb "$TMP/b.log"
    ;;
*)
    echo "unknown role: $ROLE" >&2
    exit 2
    ;;
esac
