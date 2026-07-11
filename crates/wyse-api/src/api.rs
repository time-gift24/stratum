//! HTTP-specific error mapping.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::HostError;

impl IntoResponse for HostError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::AgentNotFound { .. } | Self::TemplateNotFound { .. } => StatusCode::NOT_FOUND,
            Self::ToolNotAvailable { .. } => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        status.into_response()
    }
}
