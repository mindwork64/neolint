//! Allocation-counted proof that the SEM fingerprint budget bounds the work.
//!
//! `src/lib.rs` carries `#![forbid(unsafe_code)]`; that applies to the library
//! crate. This separate integration-test crate needs `unsafe impl GlobalAlloc`
//! to observe the comparator's allocations. The wrapper only delegates to
//! `std::alloc::System` and counts calls and requested bytes.
//!
//! What this pins: the over-budget path performs a **constant number of
//! allocation calls and constant allocated-byte volume**, i.e. no per-diagnostic
//! fingerprint materialisation. A compute-then-discard regression reintroduces
//! O(n) work and O(n) memory. Counting bytes as well as calls also catches a
//! regression that pre-reserves one growing buffer per rule: that keeps the
//! call count low while allocating O(n) bytes.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use neolint::{
    compare_semantic_diagnostics, Diagnostic, DiagnosticKind, Severity, Span,
    MAX_COMPARE_FINGERPRINTS_PER_ENGINE,
};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// SAFETY: `Counting` forwards every operation unchanged to `System` and only
// increments atomic counters. It adds no aliasing, layout, or lifetime
// requirements of its own, so `System`'s allocation guarantees carry over.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller's layout is forwarded unchanged to `System`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller's layout is forwarded unchanged to `System`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(new_size.saturating_sub(layout.size()), Ordering::Relaxed);
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

fn allocated_bytes() -> usize {
    ALLOCATED_BYTES.load(Ordering::Relaxed)
}

/// Single test by design: libtest runs each test binary's tests in parallel, and
/// the process-wide allocation counters must have no concurrent allocators.
#[test]
fn over_budget_comparison_does_not_materialise_fingerprints() {
    // Build both inputs before measuring so construction allocations are
    // excluded from the deltas.
    let near = vec![sem001(); MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1];
    let double = vec![sem001(); 2 * MAX_COMPARE_FINGERPRINTS_PER_ENGINE + 1];

    // Positive control: a stubbed counter would make every budget assertion
    // pass, so prove the counters observe a known one-byte allocation.
    let control_calls_before = allocations();
    let control_bytes_before = allocated_bytes();
    let control = std::hint::black_box(Vec::<u8>::with_capacity(1));
    drop(control);
    let control_calls = allocations() - control_calls_before;
    let control_bytes = allocated_bytes() - control_bytes_before;
    assert_eq!(
        control_calls, 1,
        "the allocation counter is not observing allocations"
    );
    assert!(
        control_bytes >= 1,
        "the byte counter is not observing allocations"
    );

    let near_calls_before = allocations();
    let near_bytes_before = allocated_bytes();
    let near_report = compare_semantic_diagnostics(&near, &[]);
    let near_calls = allocations() - near_calls_before;
    let near_bytes = allocated_bytes() - near_bytes_before;

    let double_calls_before = allocations();
    let double_bytes_before = allocated_bytes();
    let double_report = compare_semantic_diagnostics(&double, &[]);
    let double_calls = allocations() - double_calls_before;
    let double_bytes = allocated_bytes() - double_bytes_before;

    println!(
        "allocation deltas: N+1 calls={near_calls} bytes={near_bytes}; \
         2N+1 calls={double_calls} bytes={double_bytes}; \
         positive control calls={control_calls} bytes={control_bytes}"
    );

    assert!(!near_report.comparison_complete);
    assert!(!near_report.matched);
    assert!(!double_report.comparison_complete);
    assert!(!double_report.matched);

    // The incomplete path must allocate its 23-entry report skeleton; a zero
    // delta proves the measurement is broken.
    assert!(
        near_calls >= 1,
        "the incomplete path must allocate its by_rule vector"
    );
    // Constant work: both input sizes must produce exactly the same allocation
    // profile. A per-diagnostic implementation allocates at least N and 2N
    // times and cannot satisfy this equality.
    assert_eq!(
        double_calls, near_calls,
        "allocation calls grew with input size: N+1={near_calls}, 2N+1={double_calls}"
    );
    assert_eq!(
        double_bytes, near_bytes,
        "allocated bytes grew with input size: N+1={near_bytes}, 2N+1={double_bytes}"
    );
    // Constant volume: the report skeleton is well below one 64 KiB block. Any
    // per-diagnostic fingerprint storage exceeds this for 100,001 findings.
    assert!(
        near_bytes < 64 * 1024,
        "N+1 incomplete path allocated {near_bytes} bytes"
    );
    assert!(
        double_bytes < 64 * 1024,
        "2N+1 incomplete path allocated {double_bytes} bytes"
    );
}
