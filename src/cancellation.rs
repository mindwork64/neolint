//! Internal cooperative cancellation for background analysis.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A cheap, clonable cancellation signal.
#[derive(Clone, Debug, Default)]
pub(crate) struct AnalysisCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl AnalysisCancellationToken {
    #[allow(dead_code)] // Used by the optional LSP worker feature.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)] // Used by the optional LSP worker feature.
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloned_tokens_share_cancellation_state() {
        let token = AnalysisCancellationToken::new();
        let worker_token = token.clone();
        assert!(!worker_token.is_cancelled());
        token.cancel();
        assert!(worker_token.is_cancelled());
    }
}
