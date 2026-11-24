// Standalone Boost.Wave 1.90.0 probe for the Issue #109 include/cache flow.
//
// This program uses the real Boost.Wave preprocessing context and a small,
// deliberately minimal model of Firestorm's `trace_include_files` hook
// (`indra/newview/fslslpreproc.cpp:559-616` at Firestorm commit 7e44160):
//
//   * includes that match an inventory item name exactly are served from a
//     temporary on-disk cache laid out as <cache>/lslpreproc/<item name>;
//   * an uncached item models the cache-miss path: the hook materialises the
//     item, returns true so Wave skips that include for this pass, and the
//     driver restarts preprocessing with a fresh context (as Firestorm does
//     from `start_process()` at `fslslpreproc.cpp:754`);
//   * a cached item returns false, letting Wave perform its ordinary filesystem
//     include lookup against the cache directory.
//
// It is NOT the Firestorm viewer, its inventory service, its asynchronous asset
// backend or its UI. It exists to run the real Wave `#pragma once` code against
// the include/cache shape Firestorm produces.
//
// Build and run with ./run.sh (requires BOOST_ROOT pointing at a Boost 1.90.0
// source tree; see README.md).

#include <boost/filesystem.hpp>
#include <boost/wave.hpp>
#include <boost/wave/cpplexer/cpp_lex_iterator.hpp>
#include <boost/wave/cpplexer/cpp_lex_token.hpp>

#include <algorithm>
#include <cerrno>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <vector>

namespace fs = boost::filesystem;

namespace {

struct Inventory {
    std::map<std::string, std::string> items;  // exact item name -> content
};

struct PassLog {
    std::vector<std::string> calls;
    std::vector<std::string> deferred;
    std::vector<std::string> inventory_misses;
};

// Firestorm keeps its asset cache map on the preprocessor object, so it survives
// the restart that follows a cache miss (`fslslpreproc.cpp:753-754`,
// `cached_assetids`). The probe models that with state shared across passes.
struct CacheState {
    std::set<std::string> cached;
};

class FirestormHook : public boost::wave::context_policies::default_preprocessing_hooks {
public:
    FirestormHook(Inventory const& inventory, fs::path cache_dir, CacheState& cache)
        : inventory_(inventory), cache_dir_(cache_dir), cache_(cache) {}

    template <typename ContextT>
    bool found_include_directive(ContextT const& /*ctx*/, std::string const& filename,
                                 bool /*include_next*/) {
        // Firestorm strips the surrounding quotes before the name lookup
        // (fslslpreproc.cpp:561) and compares the result exactly.
        std::string name = filename.substr(1, filename.size() - 2);
        log_.calls.push_back(name);
        auto item = inventory_.items.find(name);
        if (item == inventory_.items.end()) {
            log_.inventory_misses.push_back(name);
            return false;  // no inventory item: fall through to Wave's filesystem lookup
        }
        if (cache_.cached.count(name) != 0) {
            return false;  // cached: Wave resolves it from the include directory
        }
        // Cache miss: write the item under its name and defer this include.
        fs::path target = cache_dir_ / name;
        fs::create_directories(target.parent_path());
        std::ofstream out(target.string().c_str(), std::ios::binary);
        out << item->second;
        out.close();
        cache_.cached.insert(name);
        log_.deferred.push_back(name);
        return true;  // "client returned true: skip file to include"
    }

    PassLog const& log() const { return log_; }

private:
    Inventory const& inventory_;
    fs::path cache_dir_;
    CacheState& cache_;
    PassLog log_;
};

struct RunResult {
    std::string output;
    std::vector<PassLog> passes;
    int restarts = 0;
    bool error = false;
    std::string error_message;
};

RunResult preprocess(Inventory const& inventory, std::string const& root_text, fs::path const& unit_dir,
                     std::string const& root_name,
                     std::map<std::string, std::string> const& warm_cache = {}) {
    RunResult result;
    fs::path cache_dir = unit_dir / "lslpreproc";
    fs::create_directories(cache_dir);
    CacheState cache;
    // Firestorm only reaches the cached pass after its asynchronous asset
    // callback has written the file, so a scenario that needs a warm cache
    // pre-populates it here instead of relying on a synchronous write during the
    // same pass.
    for (auto const& item : warm_cache) {
        std::ofstream out((cache_dir / item.first).string().c_str(), std::ios::binary);
        out << item.second;
        out.close();
        cache.cached.insert(item.first);
    }
    const int kMaxPasses = 8;
    for (int pass = 1; pass <= kMaxPasses; ++pass) {
        std::string input = root_text;  // fresh context per pass, like Firestorm's restart
        FirestormHook hook(inventory, cache_dir, cache);
        typedef boost::wave::cpplexer::lex_token<> token_type;
        typedef boost::wave::cpplexer::lex_iterator<token_type> lex_iterator_type;
        typedef boost::wave::context<std::string::iterator, lex_iterator_type,
                                     boost::wave::iteration_context_policies::load_file_to_string,
                                     FirestormHook>
            context_type;
        context_type ctx(input.begin(), input.end(), root_name.c_str(), hook);
        ctx.add_include_path(cache_dir.string().c_str());
        std::string output;
        bool caught = false;
        try {
            context_type::iterator_type it = ctx.begin();
            context_type::iterator_type end = ctx.end();
            for (; it != end; ++it) {
                output += (*it).get_value().c_str();
            }
        } catch (boost::wave::cpp_exception const& error) {
            caught = true;
            result.error = true;
            result.error_message = error.what();
        }
        // Wave stores its own copy of the hooks object, so the call log must be
        // read back from the context rather than from the local instance.
        PassLog pass_log = ctx.get_hooks().log();
        result.passes.push_back(pass_log);
        result.output = output;
        if (caught) {
            return result;
        }
        if (pass_log.deferred.empty()) {
            result.restarts = pass - 1;
            return result;
        }
        ++result.restarts;
    }
    result.error = true;
    result.error_message = "probe exceeded its restart budget";
    return result;
}

size_t occurrences(std::string const& haystack, std::string const& needle) {
    if (needle.empty()) {
        return 0;
    }
    size_t count = 0;
    size_t position = haystack.find(needle);
    while (position != std::string::npos) {
        ++count;
        position = haystack.find(needle, position + needle.size());
    }
    return count;
}

std::string join(std::vector<std::string> const& values) {
    std::string out;
    for (size_t index = 0; index < values.size(); ++index) {
        if (index != 0) {
            out += ",";
        }
        out += values[index];
    }
    return out.empty() ? "-" : out;
}

struct Scenario {
    std::string name;
    std::string description;
    Inventory inventory;
    std::string root;
    std::map<std::string, size_t> expected;  // body marker -> expected occurrences
    std::string note;
    bool expect_error = false;
};

int passed_count = 0;
int skipped_count = 0;
int failed_count = 0;

void report(Scenario const& scenario, RunResult const& run, fs::path const& unit_dir,
            std::string const& extra_note, bool extra_checks = true) {
    std::cout << "=== " << scenario.name << "\n";
    std::cout << "  description: " << scenario.description << "\n";
    std::cout << "  unit_dir: " << unit_dir.string() << "\n";
    std::cout << "  passes: " << run.passes.size() << ", restarts: " << run.restarts << "\n";
    for (size_t index = 0; index < run.passes.size(); ++index) {
        PassLog const& log = run.passes[index];
        std::cout << "  pass " << (index + 1) << " calls=[" << join(log.calls) << "] deferred=["
                  << join(log.deferred) << "] inventory_misses=[" << join(log.inventory_misses)
                  << "]\n";
    }
    if (run.error) {
        std::cout << "  wave_error: " << run.error_message << "\n";
    }
    std::cout << "  output: " << (run.output.empty() ? "<empty>" : run.output) << "\n";
    bool ok = true;
    if (scenario.expect_error != run.error) {
        ok = false;
        std::cout << "  ERROR-MISMATCH: expected error=" << (scenario.expect_error ? "yes" : "no")
                  << " actual=" << (run.error ? "yes" : "no") << "\n";
    }
    for (auto const& expected : scenario.expected) {
        size_t actual = occurrences(run.output, expected.first);
        std::cout << "  count(" << expected.first << "): expected " << expected.second << ", actual "
                  << actual << "\n";
        if (actual != expected.second) {
            ok = false;
        }
    }
    if (!extra_note.empty()) {
        std::cout << "  note: " << extra_note << "\n";
    }
    ok = ok && extra_checks;
    std::cout << "  RESULT: " << (ok ? "PASS" : "FAIL") << "\n\n";
    if (ok) {
        ++passed_count;
    } else {
        ++failed_count;
    }
}

void report_skip(std::string const& name, std::string const& reason) {
    std::cout << "=== " << name << "\n  SKIPPED: " << reason << "\n  RESULT: SKIP\n\n";
    ++skipped_count;
}

fs::path fresh_unit_dir(fs::path const& owned_root, std::string const& name) {
    // The root itself was created by mkdtemp and is exclusively ours, so any
    // removal below stays inside it.
    fs::path dir = owned_root / name;
    fs::create_directories(dir);
    return dir;
}

}  // namespace

int main() {
    // Create an exclusively owned temporary directory. No caller-supplied path is
    // accepted, and nothing outside this directory is ever removed.
    std::string pattern = (fs::temp_directory_path() / "neolint-wave-probe-XXXXXX").string();
    std::vector<char> buffer(pattern.begin(), pattern.end());
    buffer.push_back('\0');
    char* created = mkdtemp(buffer.data());
    if (created == nullptr) {
        std::cerr << "cannot create a temporary directory: " << std::strerror(errno) << "\n";
        return 2;
    }
    fs::path base(created);

    std::cout << "BOOST_VERSION=" << BOOST_VERSION << " BOOST_LIB_VERSION=" << BOOST_LIB_VERSION
              << "\n";
    std::cout << "BOOST_WAVE_SUPPORT_PRAGMA_ONCE=" << BOOST_WAVE_SUPPORT_PRAGMA_ONCE << "\n";
    std::cout << "compiler=" << __VERSION__ << "\n\n";

    // 1. Positive control: one include of a #pragma once header.
    {
        Scenario s;
        s.name = "01_positive_control";
        s.description = "single include of a #pragma once header";
        s.inventory.items["once.lsl"] = "#pragma once\nONCE\n";
        s.root = "#include \"once.lsl\"\n";
        s.expected["ONCE"] = 1;
        fs::path dir = fresh_unit_dir(base, s.name);
        report(s, preprocess(s.inventory, s.root, dir, "root.lsl"), dir, "");
    }

    // 2. Negative control: no #pragma once, included twice.
    {
        Scenario s;
        s.name = "02_negative_control_no_pragma";
        s.description = "header without #pragma once included twice";
        s.inventory.items["plain.lsl"] = "PLAIN\n";
        s.root = "#include \"plain.lsl\"\n#include \"plain.lsl\"\n";
        s.expected["PLAIN"] = 2;
        fs::path dir = fresh_unit_dir(base, s.name);
        report(s, preprocess(s.inventory, s.root, dir, "root.lsl"), dir, "");
    }

    // 3. Direct repeated include of the cached #pragma once item.
    {
        Scenario s;
        s.name = "03_direct_repeat";
        s.description = "same #pragma once header included twice";
        s.inventory.items["once.lsl"] = "#pragma once\nONCE\n";
        s.root = "#include \"once.lsl\"\n#include \"once.lsl\"\n";
        s.expected["ONCE"] = 1;
        fs::path dir = fresh_unit_dir(base, s.name);
        report(s, preprocess(s.inventory, s.root, dir, "root.lsl"), dir, "");
    }

    // 4. Nested include.
    {
        Scenario s;
        s.name = "04_nested";
        s.description = "#pragma once header included from a parent that is included by root";
        s.inventory.items["once.lsl"] = "#pragma once\nONCE\n";
        s.inventory.items["parent.lsl"] = "#include \"once.lsl\"\nPARENT\n";
        s.root = "#include \"parent.lsl\"\n";
        s.expected["ONCE"] = 1;
        s.expected["PARENT"] = 1;
        fs::path dir = fresh_unit_dir(base, s.name);
        report(s, preprocess(s.inventory, s.root, dir, "root.lsl"), dir, "");
    }

    // 5. Diamond include.
    {
        Scenario s;
        s.name = "05_diamond";
        s.description = "two branches include the same #pragma once header";
        s.inventory.items["once.lsl"] = "#pragma once\nONCE\n";
        s.inventory.items["left.lsl"] = "#include \"once.lsl\"\nLEFT\n";
        s.inventory.items["right.lsl"] = "#include \"once.lsl\"\nRIGHT\n";
        s.root = "#include \"left.lsl\"\n#include \"right.lsl\"\n";
        s.expected["ONCE"] = 1;
        s.expected["LEFT"] = 1;
        s.expected["RIGHT"] = 1;
        fs::path dir = fresh_unit_dir(base, s.name);
        report(s, preprocess(s.inventory, s.root, dir, "root.lsl"), dir, "");
    }

    // 6. Re-entry: include, return to the root, include again.
    {
        Scenario s;
        s.name = "06_reentry";
        s.description = "include, other include, then include the same header again";
        s.inventory.items["once.lsl"] = "#pragma once\nONCE\n";
        s.inventory.items["other.lsl"] = "OTHER\n";
        s.root =
            "#include \"once.lsl\"\n#include \"other.lsl\"\n#include \"once.lsl\"\n";
        s.expected["ONCE"] = 1;
        s.expected["OTHER"] = 1;
        fs::path dir = fresh_unit_dir(base, s.name);
        report(s, preprocess(s.inventory, s.root, dir, "root.lsl"), dir, "");
    }

    // 7. Distinct #pragma once headers must not suppress each other.
    {
        Scenario s;
        s.name = "07_distinct_headers";
        s.description = "two different #pragma once headers keep their own bodies";
        s.inventory.items["alpha.lsl"] = "#pragma once\nALPHA\n";
        s.inventory.items["beta.lsl"] = "#pragma once\nBETA\n";
        s.root = "#include \"alpha.lsl\"\n#include \"beta.lsl\"\n#include \"alpha.lsl\"\n";
        s.expected["ALPHA"] = 1;
        s.expected["BETA"] = 1;
        fs::path dir = fresh_unit_dir(base, s.name);
        report(s, preprocess(s.inventory, s.root, dir, "root.lsl"), dir, "");
    }

    // 8. Cache-miss/defer lifecycle: the pragma-once check must happen on the
    // pass that opens the cached file, not on the deferring pass.
    {
        Scenario s;
        s.name = "08_cache_miss_lifecycle";
        s.description = "uncached inventory item is deferred, materialised, then opened";
        s.inventory.items["once.lsl"] = "#pragma once\nONCE\n";
        s.root = "#include \"once.lsl\"\n";
        s.expected["ONCE"] = 1;
        fs::path dir = fresh_unit_dir(base, s.name);
        RunResult run = preprocess(s.inventory, s.root, dir, "root.lsl");
        bool deferred_first = run.passes.size() >= 2 && !run.passes[0].deferred.empty();
        bool opened_second = run.passes.size() >= 2 && run.passes[1].deferred.empty();
        std::string note = deferred_first && opened_second
                               ? "deferred on pass 1, opened on pass 2"
                               : "UNEXPECTED lifecycle";
        report(s, run, dir, note, deferred_first && opened_second);
    }

    // 9a. Dot segments through Wave's filesystem lookup, cached item first.
    {
        Scenario s;
        s.name = "09a_dot_segment_warm_cache";
        s.description =
            "warm cache: plain spelling then ./ spelling, with the item already cached";
        s.inventory.items["name.lsl"] = "#pragma once\nNAME\n";
        s.root = "#include \"name.lsl\"\n#include \"./name.lsl\"\n";
        s.expected["NAME"] = 1;
        fs::path dir = fresh_unit_dir(base, s.name);
        RunResult run = preprocess(s.inventory, s.root, dir, "root.lsl",
                                   {{"name.lsl", s.inventory.items["name.lsl"]}});
        bool missed = !run.passes.empty() &&
                      std::find(run.passes.back().inventory_misses.begin(),
                                run.passes.back().inventory_misses.end(),
                                "./name.lsl") != run.passes.back().inventory_misses.end();
        report(s, run, dir,
               missed ? "inventory lookup sees './name.lsl' as a miss; Wave then resolves it "
                        "lexically to the cached file"
                      : "inventory lookup did not report a miss for './name.lsl'",
               missed);
    }

    // 9b. Dot-segment spelling without a prior plain include: the inventory hook
    // misses and the file is absent, so Wave must report a bad include.
    {
        Scenario s;
        s.name = "09b_dot_segment_uncached";
        s.description = "./ spelling of an item that has never been cached";
        s.inventory.items["name.lsl"] = "#pragma once\nNAME\n";
        s.root = "#include \"./name.lsl\"\n";
        s.expect_error = true;
        fs::path dir = fresh_unit_dir(base, s.name);
        report(s, preprocess(s.inventory, s.root, dir, "root.lsl"), dir,
               "inventory requires the exact item name; no file exists yet");
    }

    // 10. Symlink alias: lexically_normal is not canonicalisation.
    {
        Scenario s;
        s.name = "10_symlink_alias";
        s.description = "two spellings of one physical file through a symlink";
        s.root = "#include \"real.lsl\"\n#include \"alias.lsl\"\n";
        s.expected["REAL"] = 2;  // Wave normalises lexically, it does not resolve symlinks
        fs::path dir = fresh_unit_dir(base, s.name);
        fs::path cache = dir / "lslpreproc";
        fs::create_directories(cache);
        std::ofstream(cache / "real.lsl") << "#pragma once\nREAL\n";
        boost::system::error_code error;
        fs::create_symlink(cache / "real.lsl", cache / "alias.lsl", error);
        if (error) {
            report_skip(s.name, "cannot create symlink: " + error.message());
        } else {
            report(s, preprocess(s.inventory, s.root, dir, "root.lsl"), dir,
                   "lexical normalisation keeps the two names distinct");
        }
    }

    fs::remove_all(base);  // only the directory created by mkdtemp above
    std::cout << "summary: passed=" << passed_count << " skipped=" << skipped_count
              << " failed=" << failed_count << "\n";
    if (skipped_count != 0) {
        std::cout << "note: skipped scenarios did not execute; do not report them as passing\n";
    }
    return failed_count == 0 ? 0 : 1;
}
