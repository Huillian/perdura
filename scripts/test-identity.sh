#!/usr/bin/env bash
# End-to-end identity test: the identity survives a process restart, the
# identity file is private, and an invalid file is never overwritten.
set -euo pipefail

BIN="$(pwd)/target/debug/perdura"
if [ ! -x "$BIN" ]; then
    echo "missing $BIN; run: cargo build -p perdura-cli" >&2
    exit 2
fi
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
DATA="$TMP/data"

fail() {
    echo "FAIL: $1" >&2
    exit 1
}

FIRST="$("$BIN" identity --data-dir "$DATA")"
SECOND="$("$BIN" identity --data-dir "$DATA")"

echo "$FIRST" | grep -q '^status created$' || fail "first run did not create the identity"
echo "$SECOND" | grep -q '^status loaded$' || fail "second run did not load the identity"

ID_FIRST="$(echo "$FIRST" | awk '/^identity /{print $2}')"
ID_SECOND="$(echo "$SECOND" | awk '/^identity /{print $2}')"
[ -n "$ID_FIRST" ] || fail "no fingerprint printed"
[ "$ID_FIRST" = "$ID_SECOND" ] || fail "identity changed between runs"

[ "$(stat -c %a "$DATA/identity")" = "600" ] || fail "identity file mode is not 600"
[ "$(stat -c %a "$DATA")" = "700" ] || fail "data directory mode is not 700"

chmod 644 "$DATA/identity"
if "$BIN" identity --data-dir "$DATA" >/dev/null 2>&1; then
    fail "accepted an identity file accessible by others"
fi
chmod 600 "$DATA/identity"

printf 'garbage' >"$DATA/identity"
if "$BIN" identity --data-dir "$DATA" >/dev/null 2>&1; then
    fail "accepted a malformed identity file"
fi
[ "$(cat "$DATA/identity")" = "garbage" ] || fail "malformed identity file was overwritten"

echo "PASS: identity persists and the file is private"
