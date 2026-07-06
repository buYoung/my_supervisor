//! Translate `application::AppError` into the uniform §5.4 error envelope.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use my_supervisor_application::AppError;
use my_supervisor_shared::error::ErrorBody;

/// Newtype so `?` in handlers converts an `AppError` into an HTTP response.
pub struct HttpError(pub AppError);

impl From<AppError> for HttpError {
    fn from(e: AppError) -> Self {
        HttpError(e)
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = ErrorBody::new(self.0.code(), self.0.to_string());
        (status, Json(body)).into_response()
    }
}
