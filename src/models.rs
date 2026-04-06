use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────
// Domain models
// ─────────────────────────────────────────────────────────────────────

/// A logged HTTP request.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RequestLog {
    pub id: i64,
    pub method: String,
    pub path: String,
    pub headers: Option<String>,
    pub body: Option<String>,
    pub created_at: String,
}

/// A logged HTTP response, linked to a request via `request_id`.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ResponseLog {
    pub id: i64,
    pub request_id: i64,
    pub status: i64,
    pub body: Option<String>,
    pub created_at: String,
}

/// A request together with its associated response — returned by the
/// detail endpoint `GET /requests/{id}`.
#[derive(Debug, Serialize)]
pub struct RequestDetail {
    #[serde(flatten)]
    pub request: RequestLog,
    pub response: Option<ResponseLog>,
}

// ─────────────────────────────────────────────────────────────────────
// Query / pagination helpers
// ─────────────────────────────────────────────────────────────────────

/// Query-string parameters for `GET /requests`.
///
/// Examples:
///   /requests?page=2&per_page=20
///   /requests?method=POST
///   /requests?path=/api/users
///   /requests?method=GET&path=/health&page=1&per_page=5
#[derive(Debug, Deserialize)]
pub struct RequestQuery {
    /// Filter by HTTP method (exact, case-insensitive).
    pub method: Option<String>,
    /// Filter by request path (substring match).
    pub path: Option<String>,
    /// Page number (1-indexed, default = 1).
    pub page: Option<i64>,
    /// Items per page (default = 20, max = 100).
    pub per_page: Option<i64>,
}

impl RequestQuery {
    /// Clamped, 1-indexed page number.
    pub fn page(&self) -> i64 {
        self.page.unwrap_or(1).max(1)
    }

    /// Clamped per-page size.
    pub fn per_page(&self) -> i64 {
        self.per_page.unwrap_or(20).clamp(1, 100)
    }

    /// SQL OFFSET derived from page + per_page.
    pub fn offset(&self) -> i64 {
        (self.page() - 1) * self.per_page()
    }
}

/// Wrapper for paginated list responses.
#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    pub pagination: PaginationMeta,
}

/// Pagination metadata included alongside every list response.
#[derive(Debug, Serialize)]
pub struct PaginationMeta {
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
    pub total_pages: i64,
}
