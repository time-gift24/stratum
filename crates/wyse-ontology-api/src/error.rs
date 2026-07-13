//! HTTP error translation at the ontology API boundary.

use std::{error::Error as StdError, fmt};

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use wyse_ontology::OntologyError;

/// JSON error returned by schema routes.
#[derive(Debug)]
pub(crate) enum ApiError {
    BadRequest(String),
    Conflict,
    Ontology(OntologyError),
}

impl From<OntologyError> for ApiError {
    fn from(value: OntologyError) -> Self {
        Self::Ontology(value)
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a [String]>,
}

struct OperationalContext {
    category: &'static str,
    resource_kind: Option<&'static str>,
    resource_id: Option<String>,
}

fn operational_context(error: &OntologyError) -> OperationalContext {
    let (category, resource_kind, resource_id) = match error {
        OntologyError::InvalidDraftName => ("invalid_draft_name", None, None),
        OntologyError::InvalidTagName => ("invalid_tag_name", None, None),
        OntologyError::InvalidRevisionId => ("invalid_revision_id", None, None),
        OntologyError::RevisionIdentityMismatch { actual, .. } => (
            "revision_identity_mismatch",
            Some("revision"),
            Some(actual.to_string()),
        ),
        OntologyError::SchemaInvalid { .. } => ("schema_invalid", None, None),
        OntologyError::ValueInvalid { .. } => ("value_invalid", None, None),
        OntologyError::PublishInvalid { .. } => ("publish_invalid", None, None),
        OntologyError::DraftMissing { name } => {
            ("draft_missing", Some("draft"), Some(name.to_string()))
        }
        OntologyError::DraftConflict { name } => {
            ("draft_conflict", Some("draft"), Some(name.to_string()))
        }
        OntologyError::DraftCasUnsupported => ("draft_cas_unsupported", None, None),
        OntologyError::RevisionMissing { id } => {
            ("revision_missing", Some("revision"), Some(id.to_string()))
        }
        OntologyError::TagMissing { name } => ("tag_missing", Some("tag"), Some(name.to_string())),
        OntologyError::ReservedTag => ("reserved_tag", Some("tag"), Some("online".to_owned())),
        OntologyError::OnlineRevisionChanged => (
            "online_revision_changed",
            Some("tag"),
            Some("online".to_owned()),
        ),
        OntologyError::ObjectMissing { id } => {
            ("object_missing", Some("object"), Some(id.to_string()))
        }
        OntologyError::ObjectVersionConflict { id } => (
            "object_version_conflict",
            Some("object"),
            Some(id.to_string()),
        ),
        OntologyError::ObjectReferenced { id } => {
            ("object_referenced", Some("object"), Some(id.to_string()))
        }
        OntologyError::ObjectTypeMissing { id } => (
            "object_type_missing",
            Some("object_type"),
            Some(id.to_string()),
        ),
        OntologyError::PropertyTypeMissing { id, .. } => (
            "property_type_missing",
            Some("property_type"),
            Some(id.to_string()),
        ),
        OntologyError::LinkMissing { id } => ("link_missing", Some("link"), Some(id.to_string())),
        OntologyError::LinkVersionConflict { id } => {
            ("link_version_conflict", Some("link"), Some(id.to_string()))
        }
        OntologyError::LinkTypeMissing { id } => {
            ("link_type_missing", Some("link_type"), Some(id.to_string()))
        }
        OntologyError::LinkEndpointInvalid { .. } => ("link_endpoint_invalid", None, None),
        OntologyError::CardinalityConflict { link_type_id } => (
            "cardinality_conflict",
            Some("link_type"),
            Some(link_type_id.to_string()),
        ),
        OntologyError::InvalidPageLimit { .. } => ("invalid_page_limit", None, None),
        OntologyError::Repository(_) => ("repository", None, None),
        OntologyError::DecodeSchema(_) => ("decode_schema", None, None),
        OntologyError::EncodeSchema(_) => ("encode_schema", None, None),
        OntologyError::Filesystem(_) => ("filesystem", None, None),
    };
    OperationalContext {
        category,
        resource_kind,
        resource_id,
    }
}

struct ErrorChain<'a>(&'a (dyn StdError + 'static));

impl fmt::Display for ErrorChain<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)?;
        let mut source = self.0.source();
        while let Some(error) = source {
            write!(formatter, ": {error}")?;
            source = error.source();
        }
        Ok(())
    }
}

fn log_internal_error(error: &OntologyError, status: StatusCode) {
    let context = operational_context(error);
    if matches!(error, OntologyError::Repository(_)) {
        tracing::error!(
            http.status_code = status.as_u16(),
            error.category = context.category,
            error.code = "repository_failure",
            resource.kind = context.resource_kind.unwrap_or(""),
            resource.id = context.resource_id.as_deref().unwrap_or(""),
            "ontology HTTP request failed"
        );
    } else {
        tracing::error!(
            http.status_code = status.as_u16(),
            error.category = context.category,
            error.code = context.category,
            resource.kind = context.resource_kind.unwrap_or(""),
            resource.id = context.resource_id.as_deref().unwrap_or(""),
            error.chain = %ErrorChain(error),
            "ontology HTTP request failed"
        );
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error, message, diagnostics) = match &self {
            Self::BadRequest(message) => (
                StatusCode::BAD_REQUEST,
                "bad_request",
                Some(message.as_str()),
                None,
            ),
            Self::Conflict => (StatusCode::CONFLICT, "conflict", None, None),
            Self::Ontology(error) => match error {
                OntologyError::DraftMissing { .. }
                | OntologyError::RevisionMissing { .. }
                | OntologyError::TagMissing { .. }
                | OntologyError::ObjectMissing { .. }
                | OntologyError::LinkMissing { .. }
                | OntologyError::ObjectTypeMissing { .. }
                | OntologyError::PropertyTypeMissing { .. }
                | OntologyError::LinkTypeMissing { .. } => {
                    (StatusCode::NOT_FOUND, "not_found", None, None)
                }
                OntologyError::DraftConflict { .. }
                | OntologyError::ObjectVersionConflict { .. }
                | OntologyError::LinkVersionConflict { .. }
                | OntologyError::OnlineRevisionChanged => (
                    StatusCode::PRECONDITION_FAILED,
                    "precondition_failed",
                    None,
                    None,
                ),
                OntologyError::ObjectReferenced { .. }
                | OntologyError::CardinalityConflict { .. }
                | OntologyError::ReservedTag => (StatusCode::CONFLICT, "conflict", None, None),
                OntologyError::SchemaInvalid { diagnostics } => {
                    let status = if diagnostics
                        .iter()
                        .any(|item| item.starts_with("duplicate "))
                    {
                        StatusCode::CONFLICT
                    } else {
                        StatusCode::UNPROCESSABLE_ENTITY
                    };
                    (status, "schema_invalid", None, Some(diagnostics.as_slice()))
                }
                OntologyError::ValueInvalid { diagnostics }
                | OntologyError::PublishInvalid { diagnostics }
                | OntologyError::LinkEndpointInvalid { diagnostics } => (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "validation_failed",
                    None,
                    Some(diagnostics.as_slice()),
                ),
                OntologyError::InvalidDraftName
                | OntologyError::InvalidTagName
                | OntologyError::InvalidRevisionId
                | OntologyError::RevisionIdentityMismatch { .. }
                | OntologyError::InvalidPageLimit { .. } => (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_request",
                    None,
                    None,
                ),
                OntologyError::DraftCasUnsupported
                | OntologyError::Repository(_)
                | OntologyError::DecodeSchema(_)
                | OntologyError::EncodeSchema(_)
                | OntologyError::Filesystem(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    None,
                    None,
                ),
            },
        };
        if status.is_server_error()
            && let Self::Ontology(error) = &self
        {
            log_internal_error(error, status);
        }
        (
            status,
            Json(ErrorBody {
                error,
                message,
                diagnostics,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fmt::{self, Write as _},
        sync::{Arc, Mutex},
    };

    use axum::{http::StatusCode, response::IntoResponse};
    use tracing::{Event, Subscriber, field::Visit};
    use tracing_subscriber::{Layer, layer::Context, prelude::*};
    use uuid::Uuid;
    use wyse_ontology::{ObjectId, OntologyError};

    use super::{ApiError, operational_context};

    const SENTINEL_CREDENTIAL: &str = "mysql://admin:pr20-sentinel-secret@database/ontology";

    #[derive(Clone)]
    struct CaptureLayer {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl<S> Layer<S> for CaptureLayer
    where
        S: Subscriber,
    {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            let mut fields = String::new();
            event.record(&mut FieldVisitor {
                fields: &mut fields,
            });
            self.events
                .lock()
                .expect("capture mutex is not poisoned")
                .push(fields);
        }
    }

    struct FieldVisitor<'a> {
        fields: &'a mut String,
    }

    impl Visit for FieldVisitor<'_> {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
            write!(self.fields, "{}={value:?};", field.name())
                .expect("writing to a String cannot fail");
        }
    }

    #[test]
    fn operational_context_uses_only_safe_structured_identifiers() {
        let id = ObjectId::from(Uuid::from_u128(7));
        let expected_id = id.to_string();

        let context = operational_context(&OntologyError::ObjectMissing { id });

        assert_eq!(context.category, "object_missing");
        assert_eq!(context.resource_kind, Some("object"));
        assert_eq!(context.resource_id.as_deref(), Some(expected_id.as_str()));
    }

    #[test]
    fn repository_error_tracing_redacts_arbitrary_sources() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(CaptureLayer {
            events: Arc::clone(&events),
        });

        tracing::subscriber::with_default(subscriber, || {
            let response =
                ApiError::Ontology(OntologyError::Repository(SENTINEL_CREDENTIAL.into()))
                    .into_response();

            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        });

        let events = events.lock().expect("capture mutex is not poisoned");
        assert_eq!(events.len(), 1, "expected one boundary error event");
        let event = &events[0];
        assert!(
            !event.contains(SENTINEL_CREDENTIAL),
            "captured sensitive repository detail: {event}"
        );
        assert!(event.contains("http.status_code=500;"));
        assert!(event.contains("error.category=\"repository\";"));
        assert!(event.contains("error.code=\"repository_failure\";"));
    }
}
