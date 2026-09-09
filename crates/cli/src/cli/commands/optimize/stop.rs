//! Typed operational stops are not evaluator failures or completion.

#[derive(Debug, thiserror::Error)]
#[error("optimization resource limit reached{detail}")]
pub(super) struct ResourceExhausted {
    pub uncertain: bool,
    pub detail: &'static str,
}

#[derive(Debug, thiserror::Error)]
#[error("optimization cancelled; in-flight external effects may require reconciliation")]
pub(super) struct Cancelled;
