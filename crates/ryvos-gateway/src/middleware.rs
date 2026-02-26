use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;

use crate::auth::{self, AuthResult};
use crate::state::AppState;

/// Extractor that validates authentication via Bearer header or query params.
pub struct Authenticated(pub AuthResult);

impl FromRequestParts<Arc<AppState>> for Authenticated {
    type Rejection = StatusCode;

    fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        let config = &state.config;

        // Extract Bearer token from Authorization header
        let bearer = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        // Extract query params
        let query = parts.uri.query().unwrap_or("");
        let query_token = auth::extract_token_from_query(query).map(|s| s.to_string());
        let query_password = auth::extract_password_from_query(query).map(|s| s.to_string());

        let result = auth::validate_auth(
            config,
            bearer.as_deref(),
            query_token.as_deref(),
            query_password.as_deref(),
        );

        async move {
            match result {
                Some(auth) => Ok(Authenticated(auth)),
                None => Err(StatusCode::UNAUTHORIZED),
            }
        }
    }
}
