//! Allocation-counted proof that the SEM fingerprint budget bounds the work.
//!
//! `src/lib.rs` carries `#![forbid(unsafe_code)]`; that applies to the library
//! crate. This separate integration-test crate needs `unsafe impl GlobalAlloc`
//! to observe the comparator's allocations. The wrapper only delegates to
//! `std::alloc::System` and counts calls.
//!
//! What this pins: *the budget is enforced before fingerprint materialisation;
//! a compute-then-discard regression reintroduces O(n) allocations and fails
//! here.* An implementation that clones and sorts every fingerprint only to
//! discard the result would allocate roughly once per diagnostic, so the
//! N+1 and 2N+1 inputs would have materially different allocation counts.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use neolint::{
    compare_semantic_diagnostics, Diagnostic, DiagnosticKind, Severity, Span,
    MAX_COMPARE_FINGERPRINTS_PER_ENGINE,
};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// SAFETY: `Counting` forwards every operation unchanged to `System` and only
// increments an atomic counter. It adds no aliasing, layout, or lifetime
// requirements of its own, so `System`'s allocation guarantees carry over.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's layout is forwarded unchanged to `System`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's layout is forwarded unchanged to `System`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's pointer, layout, and size are forwarded
        // unchanged to `System`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's pointer and layout are forwarded unchanged to
        // `System`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn sem001() -> Diagnostic {
    Diagnostic {
        code: "SEM001",
        severity: Severity::Warning,
        kind: DiagnosticKind::Semantic,
        message: "message".to_owned(),
        span: Span {
            start: 0,
            end: 1,
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 2,
        },
    }
}

fn allocations() -> usize {
    ALLOCATIONS.load(Ordering::Relaxed)
}

/// Single test by design: libtest runs each test binary's tests in parallel, and
/// the process-wide allocation counter must have no concurrent allocators.
#[test]
fn over_budget_comparison_does_not_materialise_fingerprints() {
    // Build both inputs before measuring so construction allocations are
    // excluded from the deltas.
    let near = vec![sem001(); MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1];
    let double = vec![sem001(); 2 * MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1];

    let before_near = allocations();
    let near_report = compare_semantic_diagnostics(&near, &[]);
    let near_delta = allocations() - before_near;

    let before_double = allocations();
    let double_report = compare_semantic_diagnostics(&double, &[]);
    let double_delta = allocations() - before_double;

    println!("allocation deltas: N+1={near_delta}, 2N+1={double_delta}");

    assert!(!near_report.comparison_complete);
    assert!(!near_report.matched);
    assert!(!double_report.comparison_complete);
    assert!(!double_report.matched);

    // The incomplete path only builds the 23-entry report skeleton. Anything
    // approaching one allocation per diagnostic means fingerprints were built.
    assert!(
        near_delta < 10_000,
        "N+1 incomplete path allocated {near_delta} times"
    );
    assert!(
        double_delta < 10_000,
        "2N+1 incomplete path allocated {double_delta} times"
    );
    assert!(
        double_delta.saturating_sub(near_delta) < 1_000,
        "allocation count grew with input size: N+1={near_delta}, 2N+1={double_delta}"
    );
}
