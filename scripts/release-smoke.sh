#!/usr/bin/env bash
set -euo pipefail

binary="${1:?release binary path required}"
version="${2:?package version required}"
root="${3:-.}"

help_output="$("$binary" --help)"
if [[ "$help_output" != *"USAGE:"* ]]; then
    echo "release smoke: --help did not print usage" >&2
    exit 1
fi

version_output="$("$binary" --version)"
if [[ "$version_output" != "neolint $version" ]]; then
    echo "release smoke: unexpected --version output: $version_output" >&2
    exit 1
fi

valid="$root/corpus/small/vector_math.lsl"
invalid="$root/corpus/invalid/duplicate_event.lsl"

if ! "$binary" "$valid" >/dev/null; then
    echo "release smoke: valid corpus fixture failed" >&2
    exit 1
fi

set +e
diagnostic_output="$("$binary" "$invalid" 2>&1)"
status=$?
set -e
if [[ "$status" -ne 1 ]]; then
    echo "release smoke: invalid fixture exited $status, expected 1" >&2
    exit 1
fi
if [[ "$diagnostic_output" != *"SEM020"* ]]; then
    echo "release smoke: expected SEM020 in invalid fixture output" >&2
    exit 1
fi

set +e
json_output="$("$binary" --format json "$invalid" 2>&1)"
json_status=$?
set -e
if [[ "$json_status" -ne 1 ]]; then
    echo "release smoke: JSON invocation exited $json_status, expected 1" >&2
    exit 1
fi
JSON_OUTPUT="$json_output" python3 - <<'PY'
import json
import os

payload = json.loads(os.environ["JSON_OUTPUT"])
assert isinstance(payload, list) and payload, "expected a non-empty JSON array"
first = payload[0]
for field in (
    "file", "code", "severity", "kind", "message",
    "line", "column", "end_line", "end_column", "start", "end",
):
    assert field in first, f"missing JSON field: {field}"
assert first["code"] == "SEM020", first
PY

echo "release smoke: ok"
