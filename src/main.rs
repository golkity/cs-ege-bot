mod db;
mod handlers;
mod keyboards;
mod reports;
mod states;

use std::sync::Arc;
use dotenvy::dotenv;
use teloxide::prelude::*;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::RequestError;
use teloxide::ApiError;
use tokio_cron_scheduler::{Job, JobScheduler};
use chrono::Utc;
use sqlx::Row;
use warp::Filter;

use crate::db::{init_db, DbPool};
use crate::states::{DialogueState, SubmissionType};

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub admin_id: i64,
    pub conspects_dir: String,
    pub media_groups: Arc<dashmap::DashMap<
        String,
        (Vec<String>, String, i64, (SubmissionType, String, String, String))
    >>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    pretty_env_logger::init();

    let token = std::env::var("API_TOKEN").expect("API_TOKEN required");
    let admin_id = std::env::var("ADMIN_ID").unwrap_or_else(|_| "0".into()).parse::<i64>()?;
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:bot.db".into());
    let conspects_dir = std::env::var("CONSPECTS_DIR").unwrap_or_else(|_| "conspects".into());

    let pool = init_db(&db_url).await?;
    tokio::fs::create_dir_all(&conspects_dir).await?;

    let app_state = AppState {
        pool: pool.clone(),
        admin_id,
        conspects_dir: conspects_dir.clone(),
        media_groups: Arc::new(dashmap::DashMap::new()),
    };

    tokio::spawn(async move {
        let port_str = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
        let port = port_str.parse::<u16>().unwrap_or(8080);
        let routes = warp::any().map(|| "I'm alive! Bot is running.");
        warp::serve(routes).run(([0, 0, 0, 0], port)).await;
    });

    let bot = Bot::new(token);

    let sched = JobScheduler::new().await?;

    let pool_remind = pool.clone();
    let bot_remind = bot.clone();
    sched.add(Job::new_async("0 0 18 * * *", move |_uuid, _l| {
        let pool = pool_remind.clone();
        let bot = bot_remind.clone();
        Box::pin(async move {
            let rows = sqlx::query("SELECT id FROM users").fetch_all(&pool).await;
            if let Ok(users) = rows {
                for row in users {
                    let uid: i64 = row.get("id");
                    match bot.send_message(
                        UserId(uid as u64),
                        "⏰ Напоминание: не забудьте сегодня сдать ДЗ и/или конспект."
                    ).await {
                        Ok(_) => {},
                        Err(RequestError::Api(ApiError::BotBlocked)) => {
                            let _ = sqlx::query("DELETE FROM users WHERE id = ?").bind(uid).execute(&pool).await;
                        },
                        Err(_) => {}
                    }
                }
            }
        })
    })?).await?;

    let pool_report = pool.clone();
    let bot_report = bot.clone();
    sched.add(Job::new_async("0 55 23 * * *", move |_uuid, _l| {
        let pool = pool_report.clone();
        let bot = bot_report.clone();
        Box::pin(async move {
            let date = Utc::now().format("%Y-%m-%d").to_string();

            let dz_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM submissions WHERE date = ? AND type = 'dz'")
                .bind(&date).fetch_one(&pool).await.unwrap_or(0);

            let conspect_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM submissions WHERE date = ? AND type = 'conspect'")
                .bind(&date).fetch_one(&pool).await.unwrap_or(0);

            let msg = format!("Ежедневный отчёт за {}:\nДЗ: {}\nКонспект: {}", date, dz_count, conspect_count);
            let _ = bot.send_message(UserId(admin_id as u64), msg).await;
        })
    })?).await?;

    let pool_reason = pool.clone();
    let bot_reason = bot.clone();
    sched.add(Job::new_async("0 57 23 * * *", move |_uuid, _l| {
        let pool = pool_reason.clone();
        let bot = bot_reason.clone();
        Box::pin(async move {
            let date = Utc::now().format("%Y-%m-%d").to_string();

            let query = "
                SELECT id FROM users
                WHERE id NOT IN (SELECT user_id FROM submissions WHERE date = ?)
            ";

            if let Ok(rows) = sqlx::query(query).bind(&date).fetch_all(&pool).await {
                for row in rows {
                    let uid: i64 = row.get("id");
                    let _ = sqlx::query("INSERT OR IGNORE INTO miss_reasons (user_id, date, reason) VALUES (?, ?, '')")
                        .bind(uid).bind(&date).execute(&pool).await;

                    match bot.send_message(
                        UserId(uid as u64),
                        format!("Сегодня ({}) ты ничего не сдал(а). Укажи причину пропуска (отправь текст).", date)
                    ).await {
                        Ok(_) => {},
                        Err(RequestError::Api(ApiError::BotBlocked)) => {
                            let _ = sqlx::query("DELETE FROM users WHERE id = ?").bind(uid).execute(&pool).await;
                        },
                        Err(_) => {}
                    }
                }
            }
        })
    })?).await?;

    let state_media = app_state.clone();
    let bot_media = bot.clone();

    let pool_danya = pool.clone();
    let bot_danya = bot.clone();

    sched.add(Job::new_async("0 0 */2 * * *", move |_uuid, _l| {
        let pool = pool_danya.clone();
        let bot = bot_danya.clone();
        Box::pin(async move {
            let rows = sqlx::query("SELECT id FROM users").fetch_all(&pool).await;

            if let Ok(users) = rows {
                let message_text = "Привет от Дани) Желаю удачкиии!!\n\nУ меня все хорошо, просто очень много прогаю и занят стартапом(((";

                for row in users {
                    let uid: i64 = row.get("id");
                    match bot.send_message(UserId(uid as u64), message_text).await {
                        Ok(_) => {},
                        Err(RequestError::Api(ApiError::BotBlocked)) => {
                            let _ = sqlx::query("DELETE FROM users WHERE id = ?").bind(uid).execute(&pool).await;
                        },
                        Err(_) => {}
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
        })
    })?).await?;

    sched.add(Job::new_async("1/2 * * * * *", move |_uuid, _l| {
        let state = state_media.clone();
        let bot = bot_media.clone();
        Box::pin(async move {
            let now = Utc::now().timestamp();
            let mut keys_to_process = Vec::new();

            for r in state.media_groups.iter() {
                if now - r.value().2 > 2 {
                    keys_to_process.push(r.key().clone());
                }
            }

            for key in keys_to_process {
                if let Some((_, (file_ids, caption, _, context))) = state.media_groups.remove(&key) {
                    let uid_str = key.split('|').next().unwrap_or("0");
                    let uid = uid_str.parse::<i64>().unwrap_or(0);
                    let (kind, section, topic_id, topic_title) = context;

                    let date = Utc::now().format("%Y-%m-%d").to_string();
                    let ts = Utc::now().to_rfc3339();
                    let summary = if caption.len() > 200 { format!("{}...", &caption[..197]) } else { caption.clone() };
                    let joined_files = file_ids.join(";");

                    let res = db::add_submission(
                        &state.pool, uid, &kind, &section, &topic_id, &topic_title,
                        "photo_album", &summary, &joined_files, 0, &date, &ts
                    ).await;

                    if res.is_ok() {
                        if matches!(kind, SubmissionType::Conspect) {
                            for fid in file_ids.iter() {
                                let _ = reports::save_file_to_disk(&bot, fid, &state.conspects_dir, uid, &section, &topic_id).await;
                            }
                        }

                        let type_str = match kind { SubmissionType::Dz => "ДЗ", SubmissionType::Conspect => "Конспект" };
                        let _ = bot.send_message(UserId(uid as u64), format!("Альбом принят! ({} фото)", file_ids.len())).await;

                        let _ = bot.send_message(UserId(state.admin_id as u64), format!(
                            "📸 Новый {} (АЛЬБОМ) от user_{}: {} - {}",
                            type_str, uid, topic_title, summary
                        )).await;
                    }
                }
            }
        })
    })?).await?;

    sched.start().await?;

    let handler = dptree::entry()
        .branch(Update::filter_message()
            .enter_dialogue::<Message, InMemStorage<DialogueState>, DialogueState>()
            .endpoint(handlers::message_handler))
        .branch(Update::filter_callback_query()
            .enter_dialogue::<CallbackQuery, InMemStorage<DialogueState>, DialogueState>()
            .endpoint(handlers::callback_handler));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![InMemStorage::<DialogueState>::new(), app_state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}