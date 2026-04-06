use actix_web::{get, post, web, HttpResponse, Responder};
use sqlx::SqlitePool;

use crate::diff;
use crate::models::{
    PaginatedResponse, PaginationMeta, ReplayResponse, RequestDetail, RequestLog, RequestQuery,
    ResponseLog,
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
// POST /requests/{id}/replay — re-execute a stored request
// ─────────────────────────────────────────────────────────────────────

/// Replay a previously captured request.
///
/// 1. Fetches the stored request (method, path, headers, body).
/// 2. Reconstructs the HTTP call using `reqwest`.
/// 3. Sends it to the same path on a configurable target host
///    (defaults to `http://127.0.0.1:8080`).
/// 4. Stores the new response in the DB.
/// 5. Diffs the original vs replayed response bodies (if JSON).
#[post("/requests/{id}/replay")]
pub async fn replay_request(
    pool: web::Data<SqlitePool>,
    id: web::Path<i64>,
) -> impl Responder {
    let request_id = id.into_inner();

    // ── 1. Load the stored request ───────────────────────────────────
    let stored_req = sqlx::query_as::<_, RequestLog>(
        "SELECT id, method, path, headers, body, created_at FROM requests WHERE id = ?",
    )
    .bind(request_id)
    .fetch_optional(pool.get_ref())
    .await;

    let stored_req = match stored_req {
        Ok(Some(r)) => r,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({
                "error": format!("Request {request_id} not found")
            }));
        }
        Err(e) => return HttpResponse::InternalServerError().json(ErrorBody::new(e)),
    };

    // Load original response for diffing
    let original_response = sqlx::query_as::<_, ResponseLog>(
        "SELECT id, request_id, status, body, created_at FROM responses WHERE request_id = ?",
    )
    .bind(request_id)
    .fetch_optional(pool.get_ref())
    .await
    .unwrap_or(None);

    // ── 2. Reconstruct the HTTP request ──────────────────────────────
    let target_base = "http://127.0.0.1:8080";
    let url = format!("{}{}", target_base, stored_req.path);

    let client = reqwest::Client::new();
    let method: reqwest::Method = stored_req
        .method
        .parse()
        .unwrap_or(reqwest::Method::GET);

    let mut req_builder = client.request(method, &url);

    // Restore original headers (skip host / content-length, reqwest sets those)
    if let Some(ref headers_json) = stored_req.headers {
        if let Ok(headers_map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(headers_json) {
            for (key, val) in &headers_map {
                let k = key.to_lowercase();
                if k == "host" || k == "content-length" || k == "transfer-encoding" {
                    continue;
                }
                if let Some(v) = val.as_str() {
                    req_builder = req_builder.header(key.as_str(), v);
                }
            }
        }
    }

    // Attach body if present
    if let Some(ref body) = stored_req.body {
        req_builder = req_builder.body(body.clone());
    }

    // ── 3. Send it ───────────────────────────────────────────────────
    let send_result = req_builder.send().await;

    let (replay_status, replay_body) = match send_result {
        Ok(resp) => {
            let status = resp.status().as_u16() as i64;
            let body = resp.text().await.unwrap_or_default();
            (status, body)
        }
        Err(e) => {
            return HttpResponse::BadGateway().json(serde_json::json!({
                "error": format!("Replay failed: {e}")
            }));
        }
    };

    // ── 4. Store the replayed response ───────────────────────────────
    // First insert a new request row to represent the replay
    let new_req_id = match sqlx::query(
        "INSERT INTO requests (method, path, headers, body) VALUES (?, ?, ?, ?)",
    )
    .bind(&stored_req.method)
    .bind(&stored_req.path)
    .bind(&stored_req.headers)
    .bind(&stored_req.body)
    .execute(pool.get_ref())
    .await
    {
        Ok(r) => r.last_insert_rowid(),
        Err(e) => return HttpResponse::InternalServerError().json(ErrorBody::new(e)),
    };

    if let Err(e) = sqlx::query(
        "INSERT INTO responses (request_id, status, body) VALUES (?, ?, ?)",
    )
    .bind(new_req_id)
    .bind(replay_status)
    .bind(&replay_body)
    .execute(pool.get_ref())
    .await
    {
        return HttpResponse::InternalServerError().json(ErrorBody::new(e));
    }

    // Build the replayed ResponseLog (use current time as approximation)
    let replayed_response = ResponseLog {
        id: new_req_id, // close enough for display
        request_id: new_req_id,
        status: replay_status,
        body: Some(replay_body.clone()),
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };

    // ── 5. Diff old vs new response bodies ───────────────────────────
    let diff_result = compute_diff(&original_response, &replay_body);

    HttpResponse::Ok().json(ReplayResponse {
        original_response,
        replayed_response,
        diff: diff_result,
    })
}

/// Try to parse both bodies as JSON and diff them.
/// Returns `None` if either body is missing or not valid JSON.
fn compute_diff(
    original: &Option<ResponseLog>,
    new_body: &str,
) -> Option<diff::DiffResult> {
    let old_body = original.as_ref()?.body.as_ref()?;
    let old_json: serde_json::Value = serde_json::from_str(old_body).ok()?;
    let new_json: serde_json::Value = serde_json::from_str(new_body).ok()?;
    Some(diff::diff_json(&old_json, &new_json))
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

