#!/usr/bin/env bash
# Build and run the standalone Boost.Wave 1.90.0 #pragma once probe.
#
# BOOST_ROOT must point at a Boost 1.90.0 source tree whose Boost.Filesystem is
# built in $BOOST_ROOT/stage/lib (see README.md). Nothing here is part of the
# project test gate; the probe is an optional developer evidence tool.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
: "${BOOST_ROOT:?set BOOST_ROOT to a Boost 1.90.0 source tree}"
g++ -std=c++17 -O2 -Wall -Wextra \
    -isystem "$BOOST_ROOT" \
    "$here/probe.cpp" \
    -L"$BOOST_ROOT/stage/lib" -lboost_wave -lboost_filesystem -lboost_thread -lboost_chrono -lboost_atomic -lpthread \
    -o "$here/probe"
"$here/probe"
