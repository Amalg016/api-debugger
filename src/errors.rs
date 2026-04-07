use thiserror::Error;

/// Application-level error type used across handlers and services.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("Request {0} not found")]
    NotFound(i64),

    #[error("Replay failed: {0}")]
    ReplayFailed(String),

    #[error("Bad request: {0}")]
    BadRequest(String),
}

impl actix_web::ResponseError for AppError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        use actix_web::http::StatusCode;
        match self {
            AppError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::ReplayFailed(_) => StatusCode::BAD_GATEWAY,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> actix_web::HttpResponse {
        let status = self.status_code();
        actix_web::HttpResponse::build(status).json(serde_json::json!({
            "error": self.to_string()
        }))
    }
}
