mod recording;

pub(crate) use recording::{
    ensure_current_review_dispatch_id, ensure_current_review_dispatch_id_for_command,
    ensure_review_dispatch_authoritative_bootstrap, record_review_dispatch_strategy_checkpoint,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReviewDispatchMutationAction {
    Recorded,
    AlreadyCurrent,
}
