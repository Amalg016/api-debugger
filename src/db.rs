use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::path::Path;

/// Initialize the SQLite database: create the file if needed, run migrations.
pub async fn init_db(db_url: &str) -> Result<SqlitePool, sqlx::Error> {
    // Ensure the database file exists (SQLite needs this with some drivers)
    let path = db_url.strip_prefix("sqlite:").unwrap_or(db_url);
    if !Path::new(path).exists() {
        std::fs::File::create(path).expect("Failed to create database file");
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(db_url)
        .await?;

    run_migrations(&pool).await?;

    Ok(pool)
}

/// Create the `requests` and `responses` tables if they don't exist.
async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS requests (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            method     TEXT    NOT NULL,
            path       TEXT    NOT NULL,
            headers    TEXT,
            body       TEXT,
            created_at DATETIME NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS responses (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id INTEGER NOT NULL,
            status     INTEGER NOT NULL,
            body       TEXT,
            created_at DATETIME NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (request_id) REFERENCES requests(id)
        );
        "#,
    )
    .execute(pool)
    .await?;

    println!("✅ Database tables ready.");
    Ok(())
}

/// Insert a dummy request + response for testing.
pub async fn insert_dummy_data(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let req_id = sqlx::query(
        r#"
        INSERT INTO requests (method, path, headers, body)
        VALUES ('GET', '/api/test', '{"Content-Type": "application/json"}', '{"hello": "world"}')
        "#,
    )
    .execute(pool)
    .await?
    .last_insert_rowid();

    sqlx::query(
        r#"
        INSERT INTO responses (request_id, status, body)
        VALUES (?, 200, '{"status": "ok"}')
        "#,
    )
    .bind(req_id)
    .execute(pool)
    .await?;

    println!("📦 Dummy data inserted (request id = {req_id}).");
    Ok(())
}
