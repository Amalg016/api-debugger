mod db;
mod diff;
mod errors;
mod handlers;
mod middleware;
mod models;
mod services;

use actix_web::{web, App, HttpServer};
use middleware::RequestLogger;
use tracing::info;
use tracing_actix_web::TracingLogger;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // ── Initialise tracing ───────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "api_debugger=info,actix_web=info".parse().unwrap()),
        )
        .with_target(false)
        .compact()
        .init();

    let db_url = "sqlite:api_debugger.db";

    // ── Initialise database ──────────────────────────────────────────
    let pool = db::init_db(db_url)
        .await
        .expect("Failed to initialise database");

    info!("🚀 Server starting at http://127.0.0.1:8080");

    // ── Start Actix server ───────────────────────────────────────────
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            // Structured request tracing
            .wrap(TracingLogger::default())
            // Custom logging middleware — persists traffic to DB
            .wrap(RequestLogger)
            // UI
            .service(handlers::ui::dashboard)
            // API
            .service(handlers::api::health)
            .service(handlers::api::list_requests)
            .service(handlers::api::get_request)
            .service(handlers::api::replay_request)
            .service(handlers::api::list_responses)
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
