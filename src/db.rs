use sqlx::{sqlite::{SqlitePoolOptions, SqliteConnectOptions}, Pool, Sqlite, Row};
use std::path::Path;
use std::str::FromStr;

pub type DbPool = Pool<Sqlite>;

pub async fn init_db(database_url: &str) -> Result<DbPool, sqlx::Error> {
    let connection_options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connection_options)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            username TEXT,
            first_name TEXT
        );"
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS submissions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER,
            type TEXT,
            section TEXT,
            topic_id TEXT,
            topic_title TEXT,
            content_type TEXT,
            content_summary TEXT,
            photo_file_id TEXT,
            message_id INTEGER,
            date TEXT,
            ts TEXT,
            FOREIGN KEY (user_id) REFERENCES users(id)
        );"
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS miss_reasons (
            user_id INTEGER,
            date TEXT,
            reason TEXT,
            PRIMARY KEY (user_id, date)
        );"
    ).execute(&pool).await?;

    Ok(pool)
}

pub async fn upsert_user(pool: &DbPool, id: i64, username: Option<String>, first_name: String) -> anyhow::Result<()> {
    sqlx::query("INSERT OR REPLACE INTO users (id, username, first_name) VALUES (?, ?, ?)")
        .bind(id)
        .bind(username.unwrap_or_default())
        .bind(first_name)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn add_submission(
    pool: &DbPool,
    user_id: i64,
    kind: &crate::states::SubmissionType,
    section: &str,
    topic_id: &str,
    topic_title: &str,
    content_type: &str,
    content_summary: &str,
    photo_file_id: &str,
    message_id: i32,
    date: &str,
    ts: &str
) -> anyhow::Result<()> {
    let type_str = match kind {
        crate::states::SubmissionType::Dz => "dz",
        crate::states::SubmissionType::Conspect => "conspect",
    };

    sqlx::query(
        "INSERT INTO submissions (
            user_id, type, section, topic_id, topic_title, content_type,
            content_summary, photo_file_id, message_id, date, ts
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
        .bind(user_id)
        .bind(type_str)
        .bind(section)
        .bind(topic_id)
        .bind(topic_title)
        .bind(content_type)
        .bind(content_summary)
        .bind(photo_file_id)
        .bind(message_id)
        .bind(date)
        .bind(ts)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn is_waiting_for_reason(pool: &DbPool, user_id: i64) -> anyhow::Result<bool> {
    let result = sqlx::query("SELECT 1 FROM miss_reasons WHERE user_id = ? AND reason = ''")
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(result.is_some())
}

pub async fn save_miss_reason(pool: &DbPool, user_id: i64, reason: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE miss_reasons SET reason = ? WHERE user_id = ? AND reason = ''")
        .bind(reason)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_user_fully(pool: &DbPool, conspects_dir: &str, identifier: &str) -> anyhow::Result<()> {
    let user_opt = if let Ok(id) = identifier.parse::<i64>() {
        sqlx::query("SELECT id FROM users WHERE id = ?").bind(id).fetch_optional(pool).await?
    } else {
        sqlx::query("SELECT id FROM users WHERE username = ?").bind(identifier).fetch_optional(pool).await?
    };

    let user_id = match user_opt {
        Some(row) => row.get::<i64, _>("id"),
        None => return Err(anyhow::anyhow!("User not found")),
    };

    sqlx::query("DELETE FROM submissions WHERE user_id = ?").bind(user_id).execute(pool).await?;
    sqlx::query("DELETE FROM miss_reasons WHERE user_id = ?").bind(user_id).execute(pool).await?;
    sqlx::query("DELETE FROM users WHERE id = ?").bind(user_id).execute(pool).await?;

    let user_path = format!("{}/{}", conspects_dir, user_id);
    if Path::new(&user_path).exists() {
        tokio::fs::remove_dir_all(user_path).await?;
    }

    Ok(())
}

pub async fn reset_database(pool: &DbPool) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM submissions").execute(pool).await?;
    sqlx::query("DELETE FROM miss_reasons").execute(pool).await?;
    sqlx::query("DELETE FROM users").execute(pool).await?;
    Ok(())
}