#!/usr/bin/env bash
# End-to-end Genesis test: node A discovers node B on an isolated link,
# opens an authenticated encrypted channel and exchanges one message.
# Needs: unshare (util-linux), ip (iproute2), unprivileged user namespaces.
set -euo pipefail

SELF="$(readlink -f "$0")"
ROLE="${1:-main}"
MESSAGE="hello-perdura"

ifindex() {
    ip -o link show dev "$1" | cut -d: -f1
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
    export BIN TMP MESSAGE

    FP_A="$("$BIN" identity --data-dir "$TMP/a" | awk '/^identity /{print $2}')"
    FP_B="$("$BIN" identity --data-dir "$TMP/b" | awk '/^identity /{print $2}')"

    unshare --user --map-root-user --net bash "$SELF" outer

    if [ -n "$FP_A" ] && [ -n "$FP_B" ] && [ "$FP_A" != "$FP_B" ] \
        && grep -q "^peer $FP_B\$" "$TMP/a.log" \
        && grep -q "^reply $MESSAGE\$" "$TMP/a.log" \
        && grep -q "^peer $FP_A\$" "$TMP/b.log" \
        && grep -q "^received $MESSAGE\$" "$TMP/b.log"; then
        echo "PASS: discovery, authenticated channel and message exchange"
    else
        echo "FAIL" >&2
        for name in a-discover a b-discover b; do
            echo "--- $name ---" >&2
            cat "$TMP/$name.log" >&2 2>/dev/null || true
        done
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

    "$BIN" discover --ifindex "$(ifindex va)" --duration 8 >"$TMP/a-discover.log" 2>&1 || true
    ADDRESS="$(awk '/^found /{print $3; exit}' "$TMP/a-discover.log")"
    : >"$TMP/a.log"
    if [ -n "$ADDRESS" ]; then
        "$BIN" send --data-dir "$TMP/a" --to "$ADDRESS" --message "$MESSAGE" >"$TMP/a.log" 2>&1 || true
    fi
    wait "$B_JOB" || true
    ;;
node-b)
    echo "$$" >"$TMP/b.pid"
    for _ in $(seq 1 50); do
        ip link show dev vb >/dev/null 2>&1 && break
        sleep 0.1
    done
    ip link set vb up
    "$BIN" discover --ifindex "$(ifindex vb)" --duration 12 >"$TMP/b-discover.log" 2>&1 &
    "$BIN" listen --data-dir "$TMP/b" --duration 20 >"$TMP/b.log" 2>&1 || true
    wait || true
    ;;
*)
    echo "unknown role: $ROLE" >&2
    exit 2
    ;;
esac
