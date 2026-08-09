//! Typed API errors and the stable HTTP error envelope.
//!
//! Every `/v1` error response uses the `{"error":{"code","message"}}`
//! envelope. Codes are stable snake_case discriminants; messages are fixed,
//! safe, and never contain SQL, NATS subjects, host paths, prompts, tool
//! payloads, provider bodies, or credentials. Source chains are kept inside
//! [`ApiError`]; the HTTP boundary logs only the stable safe classification,
//! never arbitrary source text: `tracing::error!` for 5xx and
//! `tracing::warn!` for 4xx.

use std::error::Error as StdError;

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use stratum_infra::AgentTailError;
use stratum_postgres::PostgresError;
use utoipa::ToSchema;

/// A persisted non-exhaustive variant that this API binary cannot map to its
/// explicit v1 behavior. These errors fail closed instead of inventing a
/// product event or approval decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PersistedVariantError {
    /// A future durable event has no explicit product-stream projection.
    #[error("persisted durable event has no v1 product projection")]
    UnsupportedDurableProductEvent,
    /// A future approval decision has no explicit v1 behavior.
    #[error("persisted approval decision is not supported")]
    UnsupportedApprovalDecision,
}

/// Failure at one concrete dispatcher scan, projection, or publish boundary.
/// Each variant preserves the typed source while exposing only a fixed safe
/// message to logs.
#[derive(Debug, thiserror::Error)]
pub(crate) enum DispatchError {
    /// NATS rejected or failed a realtime publish.
    #[error("realtime publish failed")]
    Publish(#[source] AgentTailError),
    /// Postgres failed to scan a committed durable interval.
    #[error("durable event scan failed")]
    Scan(#[source] PostgresError),
    /// A persisted event cannot be projected into the explicit v1 protocol.
    #[error("durable product projection failed")]
    Projection(#[source] PersistedVariantError),
}

/// Stable error envelope returned by every error response of the API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ErrorResponse {
    /// Error payload.
    pub error: ErrorBody,
}

/// Stable machine-readable error code plus a safe human message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ErrorBody {
    /// Stable snake_case error code.
    pub code: String,
    /// Safe human-readable message.
    pub message: String,
}

/// Stable classification of one API failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// 400: malformed body, identity, or missing required field.
    InvalidRequest,
    /// 400: cursor supplied through both channels or not parseable.
    InvalidCursor,
    /// 400: history window arguments are missing, unparsable, or inverted.
    InvalidHistoryQuery,
    /// 404: the addressed Agent does not exist.
    AgentNotFound,
    /// 404: the addressed agent template does not exist.
    TemplateNotFound,
    /// 404: the addressed approval request does not exist.
    ApprovalNotFound,
    /// 409: the idempotency key is bound to a different create request.
    IdempotencyKeyConflict,
    /// 409: the caller's expected current Turn no longer matches.
    StaleTurn,
    /// 409: the Agent has a running Turn hosted by this process.
    AgentBusy,
    /// 409: the Agent has a running Turn that is not hosted anywhere.
    ResumeRequired,
    /// 409: the requested Session differs from the bound Session.
    SessionMismatch,
    /// 409: another Agent is running on the requested Session.
    SessionBusy,
    /// 409: the exact Turn is not running.
    TurnNotRunning,
    /// 409: the exact Turn is running but not hosted by this process.
    TurnNotHosted,
    /// 409: the exact Turn still has a starting claim.
    TurnStarting,
    /// 409: the Turn committed `LoopStarted` but never its first user message.
    TurnPreambleIncomplete,
    /// 409: the approval was already resolved with the opposite decision.
    ApprovalAlreadyResolved,
    /// 409: the owning Turn reached a terminal event first.
    ApprovalInvalidated,
    /// 409: a persisted version is newer than this binary supports.
    RuntimeIncompatible,
    /// 410: the requested tail cursor was discarded by retention.
    CursorExpired,
    /// 413: the JSON body exceeded the hard limit.
    RequestTooLarge,
    /// 422: the template catalog or one template is unreadable or invalid.
    InvalidAgentTemplate,
    /// 422: the requested model is not configured.
    ModelNotConfigured,
    /// 422: provider-specific parameters failed validation.
    InvalidModelParameters,
    /// 500: durable truth is incomplete or violates an invariant.
    DurableStateCorrupt,
    /// 500: unclassified internal failure.
    Internal,
    /// 503: the Postgres execution store cannot serve the request.
    StoreUnavailable,
    /// 503: a pinned runtime component is currently unavailable.
    RuntimeUnavailable,
    /// 503: the NATS realtime tail cannot serve the subscription.
    RealtimeUnavailable,
    /// 503: the service is shutting down.
    ServiceUnavailable,
}

impl ErrorKind {
    /// Stable snake_case discriminant.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidCursor => "invalid_cursor",
            Self::InvalidHistoryQuery => "invalid_history_query",
            Self::AgentNotFound => "agent_not_found",
            Self::TemplateNotFound => "template_not_found",
            Self::ApprovalNotFound => "approval_not_found",
            Self::IdempotencyKeyConflict => "idempotency_key_conflict",
            Self::StaleTurn => "stale_turn",
            Self::AgentBusy => "agent_busy",
            Self::ResumeRequired => "resume_required",
            Self::SessionMismatch => "session_mismatch",
            Self::SessionBusy => "session_busy",
            Self::TurnNotRunning => "turn_not_running",
            Self::TurnNotHosted => "turn_not_hosted",
            Self::TurnStarting => "turn_starting",
            Self::TurnPreambleIncomplete => "turn_preamble_incomplete",
            Self::ApprovalAlreadyResolved => "approval_already_resolved",
            Self::ApprovalInvalidated => "approval_invalidated",
            Self::RuntimeIncompatible => "runtime_incompatible",
            Self::CursorExpired => "cursor_expired",
            Self::RequestTooLarge => "request_too_large",
            Self::InvalidAgentTemplate => "invalid_agent_template",
            Self::ModelNotConfigured => "model_not_configured",
            Self::InvalidModelParameters => "invalid_model_parameters",
            Self::DurableStateCorrupt => "durable_state_corrupt",
            Self::Internal => "internal_error",
            Self::StoreUnavailable => "store_unavailable",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::RealtimeUnavailable => "realtime_unavailable",
            Self::ServiceUnavailable => "service_unavailable",
        }
    }

    /// HTTP status mapped from the classification.
    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::InvalidRequest | Self::InvalidCursor | Self::InvalidHistoryQuery => {
                StatusCode::BAD_REQUEST
            }
            Self::AgentNotFound | Self::TemplateNotFound | Self::ApprovalNotFound => {
                StatusCode::NOT_FOUND
            }
            Self::IdempotencyKeyConflict
            | Self::StaleTurn
            | Self::AgentBusy
            | Self::ResumeRequired
            | Self::SessionMismatch
            | Self::SessionBusy
            | Self::TurnNotRunning
            | Self::TurnNotHosted
            | Self::TurnStarting
            | Self::TurnPreambleIncomplete
            | Self::ApprovalAlreadyResolved
            | Self::ApprovalInvalidated
            | Self::RuntimeIncompatible => StatusCode::CONFLICT,
            Self::CursorExpired => StatusCode::GONE,
            Self::RequestTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::InvalidAgentTemplate
            | Self::ModelNotConfigured
            | Self::InvalidModelParameters => StatusCode::UNPROCESSABLE_ENTITY,
            Self::DurableStateCorrupt | Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::StoreUnavailable | Self::RuntimeUnavailable | Self::RealtimeUnavailable => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Fixed safe message; never carries internal detail.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidRequest => "the request is invalid",
            Self::InvalidCursor => "the event cursor is invalid",
            Self::InvalidHistoryQuery => "the history query is invalid",
            Self::AgentNotFound => "the agent does not exist",
            Self::TemplateNotFound => "the agent template does not exist",
            Self::ApprovalNotFound => "the approval request does not exist",
            Self::IdempotencyKeyConflict => {
                "the idempotency key is already bound to a different create request"
            }
            Self::StaleTurn => "the expected current turn no longer matches the agent",
            Self::AgentBusy => "the agent already has a running turn",
            Self::ResumeRequired => "the agent has a running turn that must be resumed explicitly",
            Self::SessionMismatch => "the session does not match the agent's bound session",
            Self::SessionBusy => "the session already has a running agent",
            Self::TurnNotRunning => "the turn is not running",
            Self::TurnNotHosted => "the turn is not hosted by this process",
            Self::TurnStarting => "the turn is still starting",
            Self::TurnPreambleIncomplete => {
                "the turn committed its start without its first user message and was failed"
            }
            Self::ApprovalAlreadyResolved => {
                "the approval was already resolved with a different decision"
            }
            Self::ApprovalInvalidated => "the approval was invalidated by a terminal turn event",
            Self::RuntimeIncompatible => {
                "the persisted runtime version is not supported by this binary"
            }
            Self::CursorExpired => "the event cursor is no longer retained",
            Self::RequestTooLarge => "the request body is too large",
            Self::InvalidAgentTemplate => "the agent template is invalid",
            Self::ModelNotConfigured => "the model is not configured",
            Self::InvalidModelParameters => "the model parameters are invalid",
            Self::DurableStateCorrupt => "the durable agent state is corrupt",
            Self::Internal => "an internal error occurred",
            Self::StoreUnavailable => "the execution store is unavailable",
            Self::RuntimeUnavailable => "a runtime component is unavailable",
            Self::RealtimeUnavailable => "the realtime event tail is unavailable",
            Self::ServiceUnavailable => "the service is shutting down",
        }
    }
}

/// One API failure: a stable classification plus an optional source chain
/// kept for server-side logging only.
#[derive(Debug, thiserror::Error)]
#[error("{kind}")]
pub struct ApiError {
    kind: ErrorKind,
    #[source]
    source: Option<Box<dyn StdError + Send + Sync + 'static>>,
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl ApiError {
    /// Creates an error from its stable classification.
    #[must_use]
    pub const fn new(kind: ErrorKind) -> Self {
        Self { kind, source: None }
    }

    /// Creates an error keeping its source chain for server-side logging.
    #[must_use]
    pub fn with_source(kind: ErrorKind, source: impl StdError + Send + Sync + 'static) -> Self {
        Self {
            kind,
            source: Some(Box::new(source)),
        }
    }

    /// Stable classification of this failure.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Maps a storage error to its stable API classification, keeping the
    /// source chain for the boundary log.
    #[must_use]
    pub fn from_postgres(source: PostgresError) -> Self {
        let kind = kind_of_postgres(&source);
        Self::with_source(kind, source)
    }
}

/// Stable classification of a storage error.
#[must_use]
pub(crate) fn kind_of_postgres(source: &PostgresError) -> ErrorKind {
    match source {
        PostgresError::AgentNotFound { .. } => ErrorKind::AgentNotFound,
        PostgresError::TurnNotFound { .. } => ErrorKind::DurableStateCorrupt,
        PostgresError::ApprovalNotFound { .. } => ErrorKind::ApprovalNotFound,
        PostgresError::IdempotencyKeyConflict { .. } => ErrorKind::IdempotencyKeyConflict,
        PostgresError::StaleTurn { .. } => ErrorKind::StaleTurn,
        PostgresError::AgentBusy { .. } => ErrorKind::AgentBusy,
        PostgresError::SessionMismatch { .. } => ErrorKind::SessionMismatch,
        PostgresError::SessionBusy { .. } => ErrorKind::SessionBusy,
        PostgresError::TurnNotRunning { .. } => ErrorKind::TurnNotRunning,
        PostgresError::ApprovalAlreadyResolved { .. } => ErrorKind::ApprovalAlreadyResolved,
        PostgresError::ApprovalInvalidated { .. } => ErrorKind::ApprovalInvalidated,
        PostgresError::RuntimeIncompatible { .. } => ErrorKind::RuntimeIncompatible,
        PostgresError::DurableStateCorrupt { .. } => ErrorKind::DurableStateCorrupt,
        PostgresError::Connect(_)
        | PostgresError::Migrate(_)
        | PostgresError::StoreUnavailable(_) => ErrorKind::StoreUnavailable,
        PostgresError::ApprovalAlreadyRequested { .. }
        | PostgresError::ApprovalIdConflict { .. }
        | PostgresError::InvalidCompactionPointer { .. }
        | PostgresError::InvalidCommand(_)
        | PostgresError::SequenceOverflow { .. }
        | PostgresError::EventEncode { .. } => ErrorKind::Internal,
        _ => ErrorKind::Internal,
    }
}

impl From<PostgresError> for ApiError {
    fn from(source: PostgresError) -> Self {
        Self::from_postgres(source)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = self.kind.status();
        if status.is_server_error() {
            tracing::error!(error.code = self.kind.code(), "request failed");
        } else {
            tracing::warn!(error.code = self.kind.code(), "request rejected");
        }
        let body = ErrorResponse {
            error: ErrorBody {
                code: self.kind.code().to_owned(),
                message: self.kind.message().to_owned(),
            },
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_maps_to_its_documented_status_and_code() {
        let expectations = [
            (ErrorKind::InvalidRequest, 400, "invalid_request"),
            (ErrorKind::InvalidCursor, 400, "invalid_cursor"),
            (ErrorKind::InvalidHistoryQuery, 400, "invalid_history_query"),
            (ErrorKind::AgentNotFound, 404, "agent_not_found"),
            (ErrorKind::TemplateNotFound, 404, "template_not_found"),
            (ErrorKind::ApprovalNotFound, 404, "approval_not_found"),
            (
                ErrorKind::IdempotencyKeyConflict,
                409,
                "idempotency_key_conflict",
            ),
            (ErrorKind::StaleTurn, 409, "stale_turn"),
            (ErrorKind::AgentBusy, 409, "agent_busy"),
            (ErrorKind::ResumeRequired, 409, "resume_required"),
            (ErrorKind::SessionMismatch, 409, "session_mismatch"),
            (ErrorKind::SessionBusy, 409, "session_busy"),
            (ErrorKind::TurnNotRunning, 409, "turn_not_running"),
            (ErrorKind::TurnNotHosted, 409, "turn_not_hosted"),
            (ErrorKind::TurnStarting, 409, "turn_starting"),
            (
                ErrorKind::TurnPreambleIncomplete,
                409,
                "turn_preamble_incomplete",
            ),
            (
                ErrorKind::ApprovalAlreadyResolved,
                409,
                "approval_already_resolved",
            ),
            (ErrorKind::ApprovalInvalidated, 409, "approval_invalidated"),
            (ErrorKind::RuntimeIncompatible, 409, "runtime_incompatible"),
            (ErrorKind::CursorExpired, 410, "cursor_expired"),
            (ErrorKind::RequestTooLarge, 413, "request_too_large"),
            (
                ErrorKind::InvalidAgentTemplate,
                422,
                "invalid_agent_template",
            ),
            (ErrorKind::ModelNotConfigured, 422, "model_not_configured"),
            (
                ErrorKind::InvalidModelParameters,
                422,
                "invalid_model_parameters",
            ),
            (ErrorKind::DurableStateCorrupt, 500, "durable_state_corrupt"),
            (ErrorKind::Internal, 500, "internal_error"),
            (ErrorKind::StoreUnavailable, 503, "store_unavailable"),
            (ErrorKind::RuntimeUnavailable, 503, "runtime_unavailable"),
            (ErrorKind::RealtimeUnavailable, 503, "realtime_unavailable"),
            (ErrorKind::ServiceUnavailable, 503, "service_unavailable"),
        ];
        for (kind, status, code) in expectations {
            assert_eq!(kind.status().as_u16(), status, "status for {code}");
            assert_eq!(kind.code(), code);
            assert!(
                kind.message()
                    .chars()
                    .next()
                    .is_some_and(char::is_lowercase)
            );
            assert!(!kind.message().ends_with('.'));
        }
    }

    #[test]
    fn postgres_errors_map_to_their_stable_kinds() {
        let cases: Vec<(PostgresError, ErrorKind)> = vec![
            (
                PostgresError::AgentNotFound {
                    agent_id: stratum_core::AgentId::new(),
                },
                ErrorKind::AgentNotFound,
            ),
            (
                PostgresError::StaleTurn {
                    agent_id: stratum_core::AgentId::new(),
                    expected: None,
                    actual: None,
                },
                ErrorKind::StaleTurn,
            ),
            (
                PostgresError::ApprovalInvalidated {
                    approval_id: stratum_core::ApprovalId::new(),
                },
                ErrorKind::ApprovalInvalidated,
            ),
        ];
        for (source, kind) in cases {
            let error = ApiError::from_postgres(source);
            assert_eq!(error.kind(), kind);
            assert!(std::error::Error::source(&error).is_some());
        }
    }
}
