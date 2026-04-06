use actix_web::{get, web, HttpResponse, Responder};
use sqlx::SqlitePool;

use crate::models::{
    PaginatedResponse, PaginationMeta, RequestDetail, RequestLog, RequestQuery, ResponseLog,
};

// ─────────────────────────────────────────────────────────────────────
// Health
// ─────────────────────────────────────────────────────────────────────

#[get("/health")]
pub async fn health() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

// ─────────────────────────────────────────────────────────────────────
// GET /requests — paginated + filterable list
// ─────────────────────────────────────────────────────────────────────

/// List logged requests with optional filters and pagination.
///
/// Query params:
///   - `method`   — exact match, case-insensitive (e.g. `GET`, `post`)
///   - `path`     — substring match (e.g. `/api`)
///   - `page`     — 1-indexed page number (default 1)
///   - `per_page` — items per page, 1–100 (default 20)
#[get("/requests")]
pub async fn list_requests(
    pool: web::Data<SqlitePool>,
    query: web::Query<RequestQuery>,
) -> impl Responder {
    // ── Build WHERE clause dynamically ───────────────────────────────
    let mut conditions: Vec<String> = Vec::new();
    let mut bind_values: Vec<String> = Vec::new();

    if let Some(ref method) = query.method {
        conditions.push("UPPER(method) = UPPER(?)".into());
        bind_values.push(method.clone());
    }
    if let Some(ref path) = query.path {
        conditions.push("path LIKE '%' || ? || '%'".into());
        bind_values.push(path.clone());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // ── Count total matching rows ────────────────────────────────────
    let count_sql = format!("SELECT COUNT(*) as cnt FROM requests {where_clause}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for v in &bind_values {
        count_q = count_q.bind(v);
    }

    let total = match count_q.fetch_one(pool.get_ref()).await {
        Ok(n) => n,
        Err(e) => return HttpResponse::InternalServerError().json(ErrorBody::new(e)),
    };

    // ── Fetch the page ───────────────────────────────────────────────
    let per_page = query.per_page();
    let offset = query.offset();

    let data_sql = format!(
        "SELECT id, method, path, headers, body, created_at \
         FROM requests {where_clause} \
         ORDER BY id DESC \
         LIMIT ? OFFSET ?"
    );

    let mut data_q = sqlx::query_as::<_, RequestLog>(&data_sql);
    for v in &bind_values {
        data_q = data_q.bind(v);
    }
    data_q = data_q.bind(per_page).bind(offset);

    match data_q.fetch_all(pool.get_ref()).await {
        Ok(rows) => {
            let total_pages = (total + per_page - 1) / per_page; // ceiling division
            HttpResponse::Ok().json(PaginatedResponse {
                data: rows,
                pagination: PaginationMeta {
                    page: query.page(),
                    per_page,
                    total,
                    total_pages,
                },
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(ErrorBody::new(e)),
    }
}

// ─────────────────────────────────────────────────────────────────────
// GET /requests/{id} — single request + its response
// ─────────────────────────────────────────────────────────────────────

/// Fetch a single request by ID, along with its linked response.
#[get("/requests/{id}")]
pub async fn get_request(
    pool: web::Data<SqlitePool>,
    id: web::Path<i64>,
) -> impl Responder {
    let request_id = id.into_inner();

    // Fetch the request
    let request = sqlx::query_as::<_, RequestLog>(
        "SELECT id, method, path, headers, body, created_at FROM requests WHERE id = ?",
    )
    .bind(request_id)
    .fetch_optional(pool.get_ref())
    .await;

    let request = match request {
        Ok(Some(r)) => r,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({
                "error": format!("Request {request_id} not found")
            }));
        }
        Err(e) => return HttpResponse::InternalServerError().json(ErrorBody::new(e)),
    };

    // Fetch the linked response (may not exist yet)
    let response = sqlx::query_as::<_, ResponseLog>(
        "SELECT id, request_id, status, body, created_at FROM responses WHERE request_id = ?",
    )
    .bind(request_id)
    .fetch_optional(pool.get_ref())
    .await;

    let response = match response {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().json(ErrorBody::new(e)),
    };

    HttpResponse::Ok().json(RequestDetail { request, response })
}

// ─────────────────────────────────────────────────────────────────────
// GET /responses — kept for backwards compatibility
// ─────────────────────────────────────────────────────────────────────

#[get("/responses")]
pub async fn list_responses(pool: web::Data<SqlitePool>) -> impl Responder {
    let rows = sqlx::query_as::<_, ResponseLog>(
        "SELECT id, request_id, status, body, created_at FROM responses ORDER BY id DESC",
    )
    .fetch_all(pool.get_ref())
    .await;

    match rows {
        Ok(responses) => HttpResponse::Ok().json(responses),
        Err(e) => HttpResponse::InternalServerError().json(ErrorBody::new(e)),
    }
}

// ─────────────────────────────────────────────────────────────────────
// Error helper
// ─────────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct ErrorBody {
    error: String,
}

impl ErrorBody {
    fn new(e: impl std::fmt::Display) -> Self {
        Self {
            error: format!("DB error: {e}"),
        }
    }
}
