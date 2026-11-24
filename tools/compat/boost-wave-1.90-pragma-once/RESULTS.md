# Boost.Wave 1.90.0 `#pragma once` probe — results

Recorded evidence for the standalone probe in this directory. The probe builds
the real Boost.Wave implementation and models Firestorm's include/cache hook; it
is **not** the Firestorm viewer.

## Source and build identity

| Item | Value |
| --- | --- |
| Boost source | upstream `archives.boost.io/release/1.90.0/source/boost_1_90_0.tar.gz`, sha256 `5e93d582aff26868d581a52ae78c7d8edf3f3064742c6e77901a1f18a437eea9` |
| `BOOST_VERSION` | 109000 (`BOOST_LIB_VERSION` `1_90`) |
| `BOOST_WAVE_SUPPORT_PRAGMA_ONCE` | 1 |
| Firestorm dependency | `secondlife/3p-boost` release `v1.90.0-c7a9feb`, linux64 archive sha1 `6b93e323284bfab8ec20a7f3bc740c6915980bf8` (matches `autobuild.xml` at Firestorm `7e44160…`) |
| 3p-vs-upstream headers | **byte-identical** for every Wave header used by the probe (table below) |
| Compiler | g++ (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0, `__VERSION__` 13.3.0 |
| OS / arch | Linux 7.0.0-43-generic, x86_64 |
| Probe source | `probe.cpp` sha256 `833048da733125ab`, `run.sh` sha256 `020b2e205de92523`, `safety_check.sh` sha256 `6702c3c1d6e87ffb` |
| Build | `run.sh` (g++ 13.3.0, `-std=c++17 -O2 -Wall -Wextra`, Boost headers via `-isystem`) |
| Boost.Wave library built | `stage/lib/libboost_wave.a`, sha256 `c6cd8c1a1ed93b8b` |
| Probe binary reproducibility | not byte-reproducible across builds (GCC 13 embeds a filename-derived seed byte; two builds differed in one byte), so reproducibility is anchored on the source/header hashes and the captured output, not a binary hash |
| Captured stdout | [`captured-output.txt`](captured-output.txt) (this run, verbatim) |

Relevant header hashes (identical in upstream 1.90.0 and the Firestorm 3p package):

| Header | sha256 (first 16) |
| --- | --- |
| `boost/wave/util/cpp_iterator.hpp` | `ff9be207b60050b6` |
| `boost/wave/util/cpp_include_paths.hpp` | `358d9c6dc258fc17` |
| `boost/wave/util/filesystem_compatibility.hpp` | `dafecb1c32d47707` |
| `boost/wave/cpp_context.hpp` | `68dccb94a53677f5` |
| `boost/wave/util/interpret_pragma.hpp` | `77d3ae3a24191774` |
| `boost/wave/wave_config.hpp` | `676fa82bda3fe6e5` |
| `boost/version.hpp` | `1a3a432bf24fbd21` |

## Commands

```bash
# Boost source + libraries (Boost.Wave needs its compiled grammar library)
curl -L -o boost_1_90_0.tar.gz https://archives.boost.io/release/1.90.0/source/boost_1_90_0.tar.gz
tar -xzf boost_1_90_0.tar.gz
cd boost_1_90_0 && ./bootstrap.sh --with-libraries=filesystem,wave,thread
./b2 --with-wave --with-filesystem --with-thread -j"$(nproc)" link=static runtime-link=static stage

# Probe and its safety regression
export BOOST_ROOT=$PWD/boost_1_90_0
tools/compat/boost-wave-1.90-pragma-once/run.sh
tools/compat/boost-wave-1.90-pragma-once/safety_check.sh
```

## Scenario results

This run: **11 passed, 0 skipped, 0 failed**, exit status 0. Each scenario
compares preprocessed output body occurrences, not just the exit status. The
three outcomes are reported separately; a skipped scenario (for example when the
host cannot create a symlink) is counted as `SKIP` and is not reported as passing.

| Scenario | Passes / restarts | Expected vs actual body counts | Result |
| --- | --- | --- | --- |
| `01_positive_control` | 2, restarts: 1 | ONCE: expected 1, actual 1 | PASS |
| `02_negative_control_no_pragma` | 2, restarts: 1 | PLAIN: expected 2, actual 2 | PASS |
| `03_direct_repeat` | 2, restarts: 1 | ONCE: expected 1, actual 1 | PASS |
| `04_nested` | 3, restarts: 2 | ONCE: expected 1, actual 1; PARENT: expected 1, actual 1 | PASS |
| `05_diamond` | 3, restarts: 2 | LEFT: expected 1, actual 1; ONCE: expected 1, actual 1; RIGHT: expected 1, actual 1 | PASS |
| `06_reentry` | 2, restarts: 1 | ONCE: expected 1, actual 1; OTHER: expected 1, actual 1 | PASS |
| `07_distinct_headers` | 2, restarts: 1 | ALPHA: expected 1, actual 1; BETA: expected 1, actual 1 | PASS |
| `08_cache_miss_lifecycle` | 2, restarts: 1 | ONCE: expected 1, actual 1 | PASS |
| `09a_dot_segment_warm_cache` | 1, restarts: 0 | NAME: expected 1, actual 1 | PASS |
| `09b_dot_segment_uncached` | 1, restarts: 0 | - | PASS |
| `10_symlink_alias` | 1, restarts: 0 | REAL: expected 2, actual 2 | PASS |

## What the probe shows

- `#pragma once` suppression works through the modelled include flow: direct
  repeat, nested include, diamond include, re-entry after returning, and distinct
  headers keeping their own bodies (scenario 7).
- The cache-miss lifecycle is observable: the hook defers on pass 1, the driver
  materialises the item under `<cache>/lslpreproc/<name>`, and the pragma-once
  test happens on the pass that opens the cached file (scenario 8).
- **Dot segments are normalised away by Wave's include locator.** Scenario 9a uses
  a deliberately **warm** cache (the item is pre-written and marked cached before
  the pass, as it would be after Firestorm's asynchronous callback), includes
  `name.lsl` then `./name.lsl`, and gets one body. This refutes the earlier claim
  that Wave keys `x.lsl` and `./x.lsl` separately. Scenario 9b shows that an
  uncached `./name.lsl` is an inventory miss with no file, so Wave raises a
  bad-include error.
- **Lexical normalisation is not canonicalisation.** Two symlink spellings of one
  physical file keep distinct pragma-once keys, so the body appears twice
  (scenario 10). This is where neolint's canonical-path guard is stricter than
  Wave.

## Model limits

- The hook writes the cache file **synchronously** and then returns `true`.
  Firestorm instead starts `getInvItemAsset` and writes from the asynchronous
  callback before `start_process()`. The probe therefore does not reproduce the
  asynchronous timing, in-flight duplicate requests or any cache invalidation
  behaviour; scenario 9a uses a warm cache precisely so it does not depend on the
  synchronous write.
- The probe validates the Wave library and the stubbed include/cache flow. It does
  not execute the Firestorm viewer, its inventory database, the asynchronous asset
  service or the UI, and it makes no claim about viewer-level behaviour.

## Safety

`probe.cpp` no longer accepts a caller-supplied base path. It creates an
exclusively owned directory with `mkdtemp` and removes only that directory.
`safety_check.sh` proves that a pre-existing directory with nested sentinel files
survives both a normal run and invocations that pass path arguments, and that no
probe temporary directory is left behind.
