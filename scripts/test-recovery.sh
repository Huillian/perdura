#!/usr/bin/env bash
# End-to-end recovery test. Node B is on the link but not accepting
# channels: node A must fail explicitly, with no reply. Then node B starts
# accepting and node A must succeed.
# Needs: unshare (util-linux), ip (iproute2), unprivileged user namespaces.
set -euo pipefail

SELF="$(readlink -f "$0")"
ROLE="${1:-main}"
MESSAGE="hello-again"

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

    unshare --user --map-root-user --net bash "$SELF" outer

    if [ "$(cat "$TMP/first.status")" = "failure" ] \
        && grep -q '^error: ' "$TMP/first.log" \
        && ! grep -q '^reply ' "$TMP/first.log" \
        && grep -q "^reply $MESSAGE\$" "$TMP/second.log" \
        && grep -q "^received $MESSAGE\$" "$TMP/b.log"; then
        echo "PASS: explicit failure while the peer is down, success after it returns"
    else
        echo "FAIL" >&2
        echo "--- first.status ---" >&2
        cat "$TMP/first.status" >&2 2>/dev/null || true
        for name in a-discover first second b-discover b; do
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
    : >"$TMP/first.log"
    : >"$TMP/second.log"
    echo "skipped" >"$TMP/first.status"

    if [ -n "$ADDRESS" ]; then
        # Phase 1: node B is present but is not accepting channels.
        if "$BIN" send --data-dir "$TMP/a" --to "$ADDRESS" --message "$MESSAGE" >"$TMP/first.log" 2>&1; then
            echo "success" >"$TMP/first.status"
        else
            echo "failure" >"$TMP/first.status"
        fi

        # Phase 2: node B starts accepting; node A retries until it works.
        touch "$TMP/go"
        for _ in $(seq 1 15); do
            if "$BIN" send --data-dir "$TMP/a" --to "$ADDRESS" --message "$MESSAGE" >"$TMP/second.log" 2>&1; then
                break
            fi
            sleep 1
        done
    fi
    touch "$TMP/go"
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
    for _ in $(seq 1 300); do
        [ -e "$TMP/go" ] && break
        sleep 0.1
    done
    "$BIN" listen --data-dir "$TMP/b" --duration 20 >"$TMP/b.log" 2>&1 || true
    wait || true
    ;;
*)
    echo "unknown role: $ROLE" >&2
    exit 2
    ;;
esac
