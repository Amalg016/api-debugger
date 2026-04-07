use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::path::Path;
use tracing::info;

/// Initialize the SQLite database: create the file if needed, run migrations.
pub async fn init_db(db_url: &str) -> Result<SqlitePool, sqlx::Error> {
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

    info!("Database tables ready");
    Ok(())
}
