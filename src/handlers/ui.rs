use actix_web::{get, HttpResponse, Responder};

/// Serves the single-page UI dashboard.
/// The entire app (timeline, details, diff viewer) is embedded in one HTML page
/// with client-side routing via hash fragments.
#[get("/")]
pub async fn dashboard() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(include_str!("../../static/index.html"))
}
