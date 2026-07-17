//! Mutate retry policy — network vs HTTP, bounded backoff (agent-reliability web-revision-retry).

/// Max send attempts per autosave flush cycle (network / transient only).
pub const MUTATE_MAX_RETRY_ATTEMPTS: u32 = 3;
/// Base backoff between retries (ms); scaled by attempt index.
pub const MUTATE_RETRY_BACKOFF_MS: u32 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutateErrorKind {
    Network,
    RevisionConflict,
    PreconditionRequired,
    PermanentClient,
    PermanentServer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintFlushAction {
    Success,
    Retry { next_attempt: u32, delay_ms: u32 },
    ReloadAndRebase,
    StopConflict,
    StopPermanent,
}

/// Classify HTTP status (not a transport failure).
pub fn classify_http_status(status: u16) -> MutateErrorKind {
    match status {
        409 => MutateErrorKind::RevisionConflict,
        428 => MutateErrorKind::PreconditionRequired,
        500..=599 => MutateErrorKind::PermanentServer,
        400..=499 => MutateErrorKind::PermanentClient,
        _ => MutateErrorKind::PermanentClient,
    }
}

/// Paint elevation batch retry policy.
pub fn paint_flush_action(
    kind: Option<MutateErrorKind>,
    attempt: u32,
    already_rebased: bool,
) -> PaintFlushAction {
    let Some(kind) = kind else {
        return PaintFlushAction::Success;
    };
    match kind {
        MutateErrorKind::RevisionConflict | MutateErrorKind::PreconditionRequired => {
            if already_rebased {
                PaintFlushAction::StopConflict
            } else {
                PaintFlushAction::ReloadAndRebase
            }
        }
        MutateErrorKind::PermanentServer | MutateErrorKind::PermanentClient => {
            PaintFlushAction::StopPermanent
        }
        MutateErrorKind::Network => {
            let next = attempt.saturating_add(1);
            if next >= MUTATE_MAX_RETRY_ATTEMPTS {
                PaintFlushAction::StopPermanent
            } else {
                PaintFlushAction::Retry {
                    next_attempt: next,
                    delay_ms: MUTATE_RETRY_BACKOFF_MS.saturating_mul(next),
                }
            }
        }
    }
}

pub fn paint_stop_message(action: PaintFlushAction) -> &'static str {
    match action {
        PaintFlushAction::StopConflict => {
            "Map changed elsewhere — reload world or discard unsaved elevation edits"
        }
        PaintFlushAction::StopPermanent => "Could not save elevation — check connection and retry",
        PaintFlushAction::ReloadAndRebase => "Map updated — rebasing unsaved cells…",
        PaintFlushAction::Retry { .. } => "Autosave retry…",
        PaintFlushAction::Success => "",
    }
}

/// Wizard land-mask stamps: no silent merge on conflict.
pub fn wizard_stamp_flush_action(
    kind: Option<MutateErrorKind>,
    attempt: u32,
) -> PaintFlushAction {
    let Some(kind) = kind else {
        return PaintFlushAction::Success;
    };
    match kind {
        MutateErrorKind::RevisionConflict | MutateErrorKind::PreconditionRequired => {
            PaintFlushAction::StopConflict
        }
        MutateErrorKind::PermanentServer | MutateErrorKind::PermanentClient => {
            PaintFlushAction::StopPermanent
        }
        MutateErrorKind::Network => {
            let next = attempt.saturating_add(1);
            if next >= MUTATE_MAX_RETRY_ATTEMPTS {
                PaintFlushAction::StopPermanent
            } else {
                PaintFlushAction::Retry {
                    next_attempt: next,
                    delay_ms: MUTATE_RETRY_BACKOFF_MS.saturating_mul(next),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_distinguishes_conflict_server_and_client() {
        assert_eq!(
            classify_http_status(409),
            MutateErrorKind::RevisionConflict
        );
        assert_eq!(
            classify_http_status(500),
            MutateErrorKind::PermanentServer
        );
        assert_eq!(
            classify_http_status(403),
            MutateErrorKind::PermanentClient
        );
    }

    #[test]
    fn network_errors_retry_until_max() {
        assert!(matches!(
            paint_flush_action(Some(MutateErrorKind::Network), 0, false),
            PaintFlushAction::Retry { next_attempt: 1, .. }
        ));
        assert!(matches!(
            paint_flush_action(Some(MutateErrorKind::Network), 1, false),
            PaintFlushAction::Retry { next_attempt: 2, .. }
        ));
        assert_eq!(
            paint_flush_action(Some(MutateErrorKind::Network), 2, false),
            PaintFlushAction::StopPermanent
        );
    }

    #[test]
    fn conflict_409_reload_once_not_infinite() {
        assert_eq!(
            paint_flush_action(Some(MutateErrorKind::RevisionConflict), 0, false),
            PaintFlushAction::ReloadAndRebase
        );
        assert_eq!(
            paint_flush_action(Some(MutateErrorKind::RevisionConflict), 0, true),
            PaintFlushAction::StopConflict
        );
    }

    #[test]
    fn permanent_500_stops_without_retry() {
        assert_eq!(
            paint_flush_action(Some(MutateErrorKind::PermanentServer), 0, false),
            PaintFlushAction::StopPermanent
        );
    }
}
