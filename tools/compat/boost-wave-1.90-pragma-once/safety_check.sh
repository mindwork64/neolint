#!/usr/bin/env bash
# Safety regression for the probe's temporary-directory handling.
#
# The probe used to accept a caller-supplied base path and call remove_all() on
# it. This check proves that a pre-existing directory and its files survive both
# a normal run and an invocation that passes a path argument.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
: "${BOOST_ROOT:?set BOOST_ROOT to a Boost 1.90.0 source tree}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

sentinel="$work/sentinel"
mkdir -p "$sentinel/nested"
printf 'keep\n' > "$sentinel/keep.txt"
printf 'keep\n' > "$sentinel/nested/deep.txt"

# Build (once) and run normally.
"$here/run.sh" > /dev/null
test -f "$sentinel/keep.txt"
test -f "$sentinel/nested/deep.txt"

# The probe ignores argv now; a path argument must not be deleted.
"$here/probe" "$sentinel" > /dev/null
test -f "$sentinel/keep.txt"
test -f "$sentinel/nested/deep.txt"
test -d "$sentinel/nested"

# A second adversarial invocation with several path-like arguments.
"$here/probe" "$sentinel" "$work" /tmp > /dev/null
test -f "$sentinel/keep.txt"

# The probe must clean up only its own mkdtemp directory.
if ls -d /tmp/neolint-wave-probe-* >/dev/null 2>&1; then
    echo "FAIL: probe left temporary directories behind" >&2
    exit 1
fi

echo "safety check: pre-existing directories preserved, probe temp dirs cleaned"
