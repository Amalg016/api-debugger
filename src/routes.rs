use actix_web::{get, web, HttpResponse, Responder};
use sqlx::SqlitePool;

/// Health-check endpoint.
#[get("/health")]
pub async fn health() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

/// List all stored requests (for quick verification).
#[get("/requests")]
pub async fn list_requests(pool: web::Data<SqlitePool>) -> impl Responder {
    let rows = sqlx::query_as::<_, RequestRow>("SELECT id, method, path, headers, body, created_at FROM requests")
        .fetch_all(pool.get_ref())
        .await;

    match rows {
        Ok(requests) => HttpResponse::Ok().json(requests),
        Err(e) => HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
}

/// List all stored responses (for quick verification).
#[get("/responses")]
pub async fn list_responses(pool: web::Data<SqlitePool>) -> impl Responder {
    let rows = sqlx::query_as::<_, ResponseRow>("SELECT id, request_id, status, body, created_at FROM responses")
        .fetch_all(pool.get_ref())
        .await;

    match rows {
        Ok(responses) => HttpResponse::Ok().json(responses),
        Err(e) => HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
}

// ── Row types for sqlx ───────────────────────────────────────────────

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct RequestRow {
    pub id: i64,
    pub method: String,
    pub path: String,
    pub headers: Option<String>,
    pub body: Option<String>,
    pub created_at: String,
}

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct ResponseRow {
    pub id: i64,
    pub request_id: i64,
    pub status: i64,
    pub body: Option<String>,
    pub created_at: String,
}
