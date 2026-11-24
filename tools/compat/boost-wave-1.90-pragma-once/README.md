# Boost.Wave 1.90.0 `#pragma once` probe

Optional developer evidence tool for Issue #109. It builds the real Boost.Wave
1.90.0 implementation and runs a small model of Firestorm's inventory/cache
include hook (`indra/newview/fslslpreproc.cpp:559-616`, `:698-754`, `:1306-1312`
at Firestorm commit `7e44160…`). It is **not** the Firestorm viewer and is **not**
part of the project test gate or CI.

## What it models

| Firestorm behaviour | Probe model |
| --- | --- |
| `findInventoryByName` compares the requested name with `item->getName()` exactly (`fslslpreproc.cpp:57-88`) | in-memory `name -> content` map with exact lookup |
| cache miss starts an async fetch and returns `true` so Wave skips the include for that pass (`:599-610`) | hook writes the item to `<cache>/lslpreproc/<name>` and returns `true` |
| the callback restarts preprocessing after caching (`:712-754`) | driver restarts with a fresh `boost::wave::context` |
| cached item is opened from `<LL_PATH_CACHE>/lslpreproc/` (`:698`, `:1312`) | `ctx.add_include_path(<unit>/lslpreproc)` |

The hook is deliberately small; it is not a copy of the Firestorm implementation.

## Prerequisites

* A Boost 1.90.0 source tree with `boost_wave`, `boost_filesystem` and
  `boost_thread` built in `$BOOST_ROOT/stage/lib`.
* `g++` with C++17.

```bash
curl -L -o boost_1_90_0.tar.gz https://archives.boost.io/release/1.90.0/source/boost_1_90_0.tar.gz
tar -xzf boost_1_90_0.tar.gz
cd boost_1_90_0
./bootstrap.sh --with-libraries=filesystem,wave,thread
./b2 --with-wave --with-filesystem --with-thread -j"$(nproc)" \
     link=static runtime-link=static stage
cd -
```

## Run

```bash
export BOOST_ROOT=/path/to/boost_1_90_0
tools/compat/boost-wave-1.90-pragma-once/run.sh
```

The program prints one block per scenario with pass/restart counts, the hook call
sequence, body-occurrence counts and `PASS`/`FAIL`, then a summary line. It exits
non-zero if any scenario fails. Recorded output and provenance are in
[`RESULTS.md`](RESULTS.md).

## Scope

The probe validates the Wave library and the stubbed include/cache flow. It does
not exercise the Firestorm viewer, its inventory database, its asynchronous asset
service or its UI, and it must not be described as an end-to-end Firestorm test.
