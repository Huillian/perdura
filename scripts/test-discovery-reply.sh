#!/usr/bin/env bash
# End-to-end test of direct discovery replies. Node B drops all incoming
# multicast, like a phone whose Wi-Fi filters it. Both nodes must still
# discover each other, and node B must have learned about node A only
# through direct replies.
# Needs: unshare (util-linux), ip (iproute2), nft (nftables),
# unprivileged user namespaces.
set -euo pipefail

SELF="$(readlink -f "$0")"
ROLE="${1:-main}"
DURATION=15

node() {
    local dev="$1" log="$2" index
    index="$(ip -o link show dev "$dev" | cut -d: -f1)"
    "$BIN" discover --ifindex "$index" --duration "$DURATION" >"$log" 2>&1
}

# stat <log> <field>: value of a counter in the "stats" line.
stat_of() {
    sed -n "s/.*[ {]$2: \([0-9]*\).*/\1/p" "$1" | tail -n 1
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
    : >"$TMP/a.log"
    : >"$TMP/b.log"
    : >"$TMP/b-setup.log"

    unshare --user --map-root-user --net bash "$SELF" outer

    A_ID="$(awk '/^node /{print $2}' "$TMP/a.log")"
    B_ID="$(awk '/^node /{print $2}' "$TMP/b.log")"
    B_RECEIVED="$(stat_of "$TMP/b.log" beacons_received)"
    B_REPLIES="$(stat_of "$TMP/b.log" replies_received)"

    if [ -n "$A_ID" ] && [ -n "$B_ID" ] \
        && grep -q "^found $B_ID " "$TMP/a.log" \
        && grep -q "^found $A_ID " "$TMP/b.log" \
        && [ "${B_REPLIES:-0}" -gt 0 ] \
        && [ "${B_RECEIVED:-x}" = "${B_REPLIES:-y}" ]; then
        echo "PASS: node without multicast reception was discovered and discovered its peer"
    else
        echo "FAIL" >&2
        for name in a b b-setup; do
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
    {
        nft add table ip6 perdura_test
        nft add chain ip6 perdura_test input '{ type filter hook input priority 0 ; }'
        nft add rule ip6 perdura_test input ip6 daddr ff12::7065:7264 drop
    } >"$TMP/b-setup.log" 2>&1
    node vb "$TMP/b.log"
    ;;
*)
    echo "unknown role: $ROLE" >&2
    exit 2
    ;;
esac
