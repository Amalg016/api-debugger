use actix_web::{get, post, web, HttpResponse, Responder};
use sqlx::SqlitePool;
use tracing::instrument;

use crate::errors::AppError;
use crate::models::{
    PaginatedResponse, PaginationMeta, RequestDetail, RequestLog, RequestQuery, ResponseLog,
};
use crate::services;

// ─────────────────────────────────────────────────────────────────────
// Health
// ─────────────────────────────────────────────────────────────────────

#[get("/health")]
pub async fn health() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({"status": "ok"}))
}

// ─────────────────────────────────────────────────────────────────────
// GET /api/requests — paginated + filterable list
// ─────────────────────────────────────────────────────────────────────

#[get("/api/requests")]
#[instrument(skip(pool))]
pub async fn list_requests(
    pool: web::Data<SqlitePool>,
    query: web::Query<RequestQuery>,
) -> Result<HttpResponse, AppError> {
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

    let count_sql = format!("SELECT COUNT(*) as cnt FROM requests {where_clause}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for v in &bind_values {
        count_q = count_q.bind(v);
    }
    let total = count_q.fetch_one(pool.get_ref()).await?;

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

    let rows = data_q.fetch_all(pool.get_ref()).await?;
    let total_pages = (total + per_page - 1) / per_page;

    Ok(HttpResponse::Ok().json(PaginatedResponse {
        data: rows,
        pagination: PaginationMeta {
            page: query.page(),
            per_page,
            total,
            total_pages,
        },
    }))
}

// ─────────────────────────────────────────────────────────────────────
// GET /api/requests/{id}
// ─────────────────────────────────────────────────────────────────────

#[get("/api/requests/{id}")]
#[instrument(skip(pool))]
pub async fn get_request(
    pool: web::Data<SqlitePool>,
    id: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let request_id = id.into_inner();

    let request = sqlx::query_as::<_, RequestLog>(
        "SELECT id, method, path, headers, body, created_at FROM requests WHERE id = ?",
    )
    .bind(request_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or(AppError::NotFound(request_id))?;

    let response = sqlx::query_as::<_, ResponseLog>(
        "SELECT id, request_id, status, body, created_at FROM responses WHERE request_id = ?",
    )
    .bind(request_id)
    .fetch_optional(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(RequestDetail { request, response }))
}

// ─────────────────────────────────────────────────────────────────────
// POST /api/requests/{id}/replay
// ─────────────────────────────────────────────────────────────────────

#[post("/api/requests/{id}/replay")]
#[instrument(skip(pool))]
pub async fn replay_request(
    pool: web::Data<SqlitePool>,
    id: web::Path<i64>,
) -> Result<HttpResponse, AppError> {
    let result = services::replay::execute_replay(pool.get_ref(), id.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}

// ─────────────────────────────────────────────────────────────────────
// GET /api/responses
// ─────────────────────────────────────────────────────────────────────

#[get("/api/responses")]
pub async fn list_responses(pool: web::Data<SqlitePool>) -> Result<HttpResponse, AppError> {
    let rows = sqlx::query_as::<_, ResponseLog>(
        "SELECT id, request_id, status, body, created_at FROM responses ORDER BY id DESC",
    )
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(rows))
}
