mod db;
mod middleware;
mod routes;

use actix_web::{web, App, HttpServer};
use middleware::RequestLogger;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let db_url = "sqlite:api_debugger.db";

    // ── Initialise database ──────────────────────────────────────────
    let pool = db::init_db(db_url)
        .await
        .expect("Failed to initialise database");

    println!("🚀 Server starting at http://127.0.0.1:8080");

    // ── Start Actix server ───────────────────────────────────────────
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            // Logging middleware — captures every request/response
            .wrap(RequestLogger)
            .service(routes::health)
            .service(routes::list_requests)
            .service(routes::list_responses)
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
