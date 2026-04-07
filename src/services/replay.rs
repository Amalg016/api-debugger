use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::diff;
use crate::errors::AppError;
use crate::models::{ReplayResponse, RequestLog, ResponseLog};

/// Execute a replay of a previously stored request.
///
/// 1. Fetches the stored request.
/// 2. Reconstructs the HTTP call using `reqwest`.
/// 3. Sends it to the same path on the target host.
/// 4. Stores the new response in the DB.
/// 5. Diffs original vs replayed response bodies.
pub async fn execute_replay(
    pool: &SqlitePool,
    request_id: i64,
) -> Result<ReplayResponse, AppError> {
    // ── 1. Load the stored request ───────────────────────────────────
    let stored_req = sqlx::query_as::<_, RequestLog>(
        "SELECT id, method, path, headers, body, created_at FROM requests WHERE id = ?",
    )
    .bind(request_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound(request_id))?;

    // Load original response for diffing
    let original_response = sqlx::query_as::<_, ResponseLog>(
        "SELECT id, request_id, status, body, created_at FROM responses WHERE request_id = ?",
    )
    .bind(request_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    info!(request_id, method = %stored_req.method, path = %stored_req.path, "Replaying request");

    // ── 2. Reconstruct the HTTP request ──────────────────────────────
    let target_base = "http://127.0.0.1:8080";
    let url = format!("{}{}", target_base, stored_req.path);

    let client = reqwest::Client::new();
    let method: reqwest::Method = stored_req
        .method
        .parse()
        .unwrap_or(reqwest::Method::GET);

    let mut req_builder = client.request(method, &url);

    if let Some(ref headers_json) = stored_req.headers {
        if let Ok(headers_map) =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(headers_json)
        {
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

    if let Some(ref body) = stored_req.body {
        req_builder = req_builder.body(body.clone());
    }

    // ── 3. Send it ───────────────────────────────────────────────────
    let (replay_status, replay_body) = match req_builder.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16() as i64;
            let body = resp.text().await.unwrap_or_default();
            (status, body)
        }
        Err(e) => {
            warn!(error = %e, "Replay HTTP request failed");
            return Err(AppError::ReplayFailed(e.to_string()));
        }
    };

    // ── 4. Store the replayed response ───────────────────────────────
    let new_req_id = sqlx::query(
        "INSERT INTO requests (method, path, headers, body) VALUES (?, ?, ?, ?)",
    )
    .bind(&stored_req.method)
    .bind(&stored_req.path)
    .bind(&stored_req.headers)
    .bind(&stored_req.body)
    .execute(pool)
    .await?
    .last_insert_rowid();

    sqlx::query(
        "INSERT INTO responses (request_id, status, body) VALUES (?, ?, ?)",
    )
    .bind(new_req_id)
    .bind(replay_status)
    .bind(&replay_body)
    .execute(pool)
    .await?;

    let replayed_response = ResponseLog {
        id: new_req_id,
        request_id: new_req_id,
        status: replay_status,
        body: Some(replay_body.clone()),
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };

    // ── 5. Diff ──────────────────────────────────────────────────────
    let diff_result = compute_diff(&original_response, &replay_body);

    info!(
        request_id,
        new_request_id = new_req_id,
        replay_status,
        "Replay completed"
    );

    Ok(ReplayResponse {
        original_response,
        replayed_response,
        diff: diff_result,
    })
}

fn compute_diff(
    original: &Option<ResponseLog>,
    new_body: &str,
) -> Option<diff::DiffResult> {
    let old_body = original.as_ref()?.body.as_ref()?;
    let old_json: serde_json::Value = serde_json::from_str(old_body).ok()?;
    let new_json: serde_json::Value = serde_json::from_str(new_body).ok()?;
    Some(diff::diff_json(&old_json, &new_json))
}
