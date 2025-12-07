use teloxide::{
    prelude::*,
    types::{InputFile, MediaKind, MessageKind, MessageId},
};
use chrono::Utc;
use rand::seq::SliceRandom;
use log::{info, error};

use crate::{
    db,
    keyboards::{self, main_kb, sections_kb, topics_kb, admin_kb},
    reports,
    states::{DialogueState, SubmissionType},
    AppState,
};

pub type MyDialogue = Dialogue<DialogueState, teloxide::dispatching::dialogue::InMemStorage<DialogueState>>;
pub type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn get_praise() -> String {
    let phrases = vec![
        "Молодец, отличная работа!", "Здорово, так держать!", "Круто, ты справился!",
        "Умница, ДЗ принято!", "АЙ ЛЕВ", "Лёва оценил!!!", "Ты - будущий 100-балльник",
        "Имба, Леве понравится!", "Ну ты прям машина!!"
    ];
    let mut rng = rand::thread_rng();
    phrases.choose(&mut rng).unwrap_or(&"Принято!").to_string()
}

pub async fn message_handler(
    bot: Bot,
    msg: Message,
    dialogue: MyDialogue,
    state: AppState,
) -> HandlerResult {
    let user = match msg.from() {
        Some(u) => u,
        None => return Ok(()),
    };

    if let Err(e) = db::upsert_user(&state.pool, user.id.0 as i64, user.username.clone(), user.first_name.clone()).await {
        error!("Failed to update user: {:?}", e);
    }

    let text = msg.text().unwrap_or("");
    let uid = user.id.0 as i64;

    if text == "/start" || text == "/menu" || text == "📌 Главное меню" {
        dialogue.update(DialogueState::Start).await?;
        let is_admin = uid == state.admin_id;
        bot.send_message(msg.chat.id, "Привет! Я бот для сдачи ДЗ и конспектов.\nВыбери действие:")
            .reply_markup(main_kb(is_admin))
            .await?;
        return Ok(());
    }

    match dialogue.get().await? {
        Some(DialogueState::Start) | None => {
            match text {
                "📚 Сдать ДЗ" => {
                    dialogue.update(DialogueState::ChoosingSection { kind: SubmissionType::Dz }).await?;
                    bot.send_message(msg.chat.id, "Выбери раздел:")
                        .reply_markup(sections_kb())
                        .await?;
                }
                "📘 Сдать конспект" => {
                    dialogue.update(DialogueState::ChoosingSection { kind: SubmissionType::Conspect }).await?;
                    bot.send_message(msg.chat.id, "Выбери раздел:")
                        .reply_markup(sections_kb())
                        .await?;
                }
                "📁 Мои конспекты" => {
                    bot.send_message(msg.chat.id, "Архивирую твои конспекты, подожди пару секунд...").await?;
                    let zip_data = reports::archive_user_conspects(&state.conspects_dir, uid).await;
                    match zip_data {
                        Ok(data) if !data.is_empty() => {
                            bot.send_document(msg.chat.id, InputFile::memory(data).file_name("my_conspects.zip")).await?;
                        }
                        _ => {
                            bot.send_message(msg.chat.id, "У тебя пока нет сохранённых конспектов.").await?;
                        }
                    }
                }
                "🛠️ Админ-панель" => {
                    if uid == state.admin_id {
                        dialogue.update(DialogueState::AdminPanel).await?;
                        bot.send_message(msg.chat.id, "Админ-панель:").reply_markup(admin_kb()).await?;
                    } else {
                        bot.send_message(msg.chat.id, "Доступ запрещён.").await?;
                    }
                }
                _ => {
                    if db::is_waiting_for_reason(&state.pool, uid).await.unwrap_or(false) {
                        db::save_miss_reason(&state.pool, uid, text).await?;
                        bot.send_message(msg.chat.id, "Причина сохранена, спасибо.").await?;
                    }
                    else if text.to_lowercase().starts_with("дз") || text.to_lowercase().starts_with("конспект") {
                        bot.send_message(msg.chat.id, "Пожалуйста, используй меню для сдачи работ.").await?;
                    }
                }
            }
        }

        Some(DialogueState::WaitingForContent { kind, section, topic_id, topic_title }) => {
            let date = Utc::now().format("%Y-%m-%d").to_string();
            let ts = Utc::now().to_rfc3339();

            if let MessageKind::Common(common) = &msg.kind {

                if let MediaKind::Photo(media) = &common.media_kind {
                    let best_photo = media.photo.last().unwrap();
                    let file_id = best_photo.file.id.clone();
                    let caption = media.caption.clone().unwrap_or_default();
                    let summary = if caption.len() > 200 { format!("{}...", &caption[..197]) } else { caption.clone() };

                    if let Some(mg_id) = msg.media_group_id() {
                        let key = format!("{}|{}", uid, mg_id);
                        let mut entry = state.media_groups.entry(key).or_insert_with(|| {
                            (Vec::new(), caption.clone(), Utc::now().timestamp(), (kind.clone(), section.clone(), topic_id.clone(), topic_title.clone()))
                        });

                        entry.0.push(file_id.clone());
                        entry.2 = Utc::now().timestamp();
                        return Ok(());
                    }

                    db::add_submission(
                        &state.pool, uid, &kind, &section, &topic_id, &topic_title,
                        "photo", &summary, &file_id, msg.id.0, &date, &ts
                    ).await?;

                    if matches!(kind, SubmissionType::Conspect) {
                        if let Err(e) = reports::save_file_to_disk(&bot, &file_id, &state.conspects_dir, uid, &section, &topic_id).await {
                            error!("Ошибка сохранения фото: {:?}", e);
                            bot.send_message(msg.chat.id, "⚠️ Файл не удалось сохранить на диск. Попробуй еще раз.").await?;
                        } else {
                            info!("Фото сохранено для user {}", uid);
                        }
                    }

                    let praise = get_praise();
                    bot.send_message(msg.chat.id, format!("{} {}", praise, topic_title))
                        .reply_markup(main_kb(uid == state.admin_id))
                        .await?;

                    bot.send_message(UserId(state.admin_id as u64), format!("📸 Новый {} от @{}: {} - {}",
                                                                            match kind { SubmissionType::Dz => "ДЗ", SubmissionType::Conspect => "Конспект"},
                                                                            user.username.as_deref().unwrap_or("noname"),
                                                                            topic_title, summary
                    )).await?;

                    dialogue.exit().await?;
                    return Ok(());
                }

                if let MediaKind::Document(doc) = &common.media_kind {
                    let file_id = &doc.document.file.id;
                    let file_name = doc.document.file_name.clone().unwrap_or_else(|| "document".to_string());
                    let caption = doc.caption.clone().unwrap_or_else(|| file_name.clone());

                    db::add_submission(
                        &state.pool, uid, &kind, &section, &topic_id, &topic_title,
                        "document", &caption, file_id, msg.id.0, &date, &ts
                    ).await?;

                    if matches!(kind, SubmissionType::Conspect) {
                        if let Err(e) = reports::save_file_to_disk(&bot, file_id, &state.conspects_dir, uid, &section, &topic_id).await {
                            error!("Ошибка сохранения документа: {:?}", e);
                            bot.send_message(msg.chat.id, "⚠️ Ошибка сохранения файла.").await?;
                        } else {
                            info!("Документ сохранен для user {}", uid);
                        }
                    }

                    let praise = get_praise();
                    bot.send_message(msg.chat.id, format!("{} {} (Файл принят)", praise, topic_title))
                        .reply_markup(main_kb(uid == state.admin_id))
                        .await?;

                    bot.send_message(UserId(state.admin_id as u64), format!("📄 Новый {} (ФАЙЛ) от @{}: {} - {}",
                                                                            match kind { SubmissionType::Dz => "ДЗ", SubmissionType::Conspect => "Конспект"},
                                                                            user.username.as_deref().unwrap_or("noname"),
                                                                            topic_title, caption
                    )).await?;

                    dialogue.exit().await?;
                    return Ok(());
                }
            }

            if !text.is_empty() {
                let summary = if text.len() > 300 { format!("{}...", &text[..297]) } else { text.to_string() };

                db::add_submission(
                    &state.pool, uid, &kind, &section, &topic_id, &topic_title,
                    "text", &summary, "", msg.id.0, &date, &ts
                ).await?;

                if matches!(kind, SubmissionType::Conspect) {
                    if let Err(e) = reports::save_text_to_disk(text, &state.conspects_dir, uid, &section, &topic_id).await {
                        error!("Ошибка сохранения текста: {:?}", e);
                    }
                }

                let praise = get_praise();
                bot.send_message(msg.chat.id, format!("{} {}", praise, topic_title))
                    .reply_markup(main_kb(uid == state.admin_id))
                    .await?;

                bot.send_message(UserId(state.admin_id as u64), format!("✅ Новый {} от @{}: {} - {}",
                                                                        match kind { SubmissionType::Dz => "ДЗ", SubmissionType::Conspect => "Конспект"},
                                                                        user.username.as_deref().unwrap_or("noname"),
                                                                        topic_title, summary
                )).await?;

                dialogue.exit().await?;
                return Ok(());
            }

            bot.send_message(msg.chat.id, "Пожалуйста, отправь фото, файл или текст.").await?;
        }

        Some(DialogueState::AdminWaitingForExportUser) => {
            let target = text.trim().trim_start_matches('@');
            bot.send_message(msg.chat.id, "Начинаю выгрузку...").await?;

            match reports::export_user_data(&state.pool, &state.conspects_dir, target).await {
                Ok((excel, zip)) => {
                    bot.send_document(msg.chat.id, InputFile::memory(excel).file_name("submissions.xlsx")).await?;
                    if let Some(z) = zip {
                        bot.send_document(msg.chat.id, InputFile::memory(z).file_name("files.zip")).await?;
                    }
                    bot.send_message(msg.chat.id, "Готово.").reply_markup(admin_kb()).await?;
                }
                Err(_) => {
                    bot.send_message(msg.chat.id, "Пользователь не найден или ошибка.").reply_markup(admin_kb()).await?;
                }
            }
            dialogue.update(DialogueState::AdminPanel).await?;
        }

        Some(DialogueState::AdminWaitingForDeleteUser) => {
            if db::delete_user_fully(&state.pool, &state.conspects_dir, text.trim()).await.is_ok() {
                bot.send_message(msg.chat.id, "Пользователь удален.").reply_markup(admin_kb()).await?;
            } else {
                bot.send_message(msg.chat.id, "Ошибка удаления.").reply_markup(admin_kb()).await?;
            }
            dialogue.update(DialogueState::AdminPanel).await?;
        }

        _ => {}
    }

    Ok(())
}

pub async fn callback_handler(
    bot: Bot,
    q: CallbackQuery,
    dialogue: MyDialogue,
    state: AppState,
) -> HandlerResult {
    let data = match q.data {
        Some(d) => d,
        None => return Ok(()),
    };
    let uid = q.from.id.0 as i64;
    let msg_id = q.message.as_ref().map(|m| m.id).unwrap_or(MessageId(0));

    if data == "cancel" {
        dialogue.exit().await?;
        bot.send_message(q.from.id, "Операция отменена.")
            .reply_markup(main_kb(uid == state.admin_id)).await?;
        bot.answer_callback_query(q.id).await?;
        return Ok(());
    }

    if data.starts_with("sec|") {
        let section = data.split('|').nth(1).unwrap_or("").to_string();

        if let Some(DialogueState::ChoosingSection { kind }) = dialogue.get().await? {
            dialogue.update(DialogueState::ChoosingTopic { kind: kind.clone(), section: section.clone() }).await?;

            bot.edit_message_text(q.from.id, msg_id, format!("Раздел: {}\nВыбери тему:", section))
                .reply_markup(topics_kb(&section))
                .await?;
        } else {
            bot.answer_callback_query(q.id).text("Сессия истекла, начни заново").await?;
        }
        return Ok(());
    }

    if data.starts_with("topic|") {
        let parts: Vec<&str> = data.split('|').collect();
        if parts.len() < 3 { return Ok(()); }
        let section = parts[1].to_string();
        let topic_id = parts[2].to_string();

        let topic_title = keyboards::get_topic_title(&section, &topic_id).unwrap_or("Тема".to_string());

        if let Some(DialogueState::ChoosingTopic { kind, .. }) = dialogue.get().await? {
            dialogue.update(DialogueState::WaitingForContent {
                kind: kind.clone(),
                section: section,
                topic_id: topic_id,
                topic_title: topic_title.clone()
            }).await?;

            let type_str = match kind { SubmissionType::Dz => "ДЗ", SubmissionType::Conspect => "конспект" };
            bot.edit_message_text(q.from.id, msg_id,
                                  format!("Тема: {}\nОтправь {} (фото, файл или текст).", topic_title, type_str))
                .reply_markup(teloxide::types::InlineKeyboardMarkup::default())
                .await?;
        }
        return Ok(());
    }

    if data.starts_with("admin|") && uid == state.admin_id {
        let action = data.split('|').nth(1).unwrap_or("");

        match action {
            "daily_full" | "send_daily_now" => {
                bot.answer_callback_query(&q.id).text("Генерирую отчет...").await?;
                let date = Utc::now().format("%Y-%m-%d").to_string();
                match reports::generate_daily_report(&state.pool, &date).await {
                    Ok(excel) => {
                        bot.send_document(q.from.id, InputFile::memory(excel).file_name(format!("report_{}.xlsx", date))).await?;
                    },
                    Err(e) => {
                        error!("Report error: {:?}", e);
                        bot.send_message(q.from.id, "Ошибка генерации отчета").await?;
                    }
                }
            }

            "full_history_manual" => {
                bot.answer_callback_query(&q.id).text("Это может занять время...").await?;
                if let Ok(files) = reports::generate_full_history_package(&state.pool).await {
                    for file in files {
                        bot.send_document(q.from.id, file).await?;
                    }
                }
            }
            "export_user" => {
                dialogue.update(DialogueState::AdminWaitingForExportUser).await?;
                bot.send_message(q.from.id, "Пришли ID или @username пользователя:").await?;
            }
            "delete_user" => {
                dialogue.update(DialogueState::AdminWaitingForDeleteUser).await?;
                bot.send_message(q.from.id, "Пришли ID или @username для УДАЛЕНИЯ:").await?;
            }
            "reset_all" => {
                db::reset_database(&state.pool).await?;
                let _ = tokio::fs::remove_dir_all(&state.conspects_dir).await;
                let _ = tokio::fs::create_dir_all(&state.conspects_dir).await;
                bot.answer_callback_query(&q.id).text("База сброшена!").show_alert(true).await?;
            }
            _ => {}
        }
        bot.answer_callback_query(q.id).await?;
    }

    Ok(())
}