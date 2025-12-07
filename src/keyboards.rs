use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};

pub fn main_kb(is_admin: bool) -> KeyboardMarkup {
    let mut rows = vec![
        vec![KeyboardButton::new("📚 Сдать ДЗ"), KeyboardButton::new("📘 Сдать конспект")],
        vec![KeyboardButton::new("📁 Мои конспекты"), KeyboardButton::new("📌 Главное меню")],
    ];
    if is_admin {
        rows.push(vec![KeyboardButton::new("🛠️ Админ-панель")]);
    }
    KeyboardMarkup::new(rows).resize_keyboard(true)
}

pub fn sections_kb() -> InlineKeyboardMarkup {
    let sections = vec!["Основы Питона", "ЕГЭ 1-27"];
    let mut buttons = vec![];
    for sec in sections {
        buttons.push(vec![InlineKeyboardButton::callback(sec, format!("sec|{}", sec))]);
    }
    buttons.push(vec![InlineKeyboardButton::callback("Отмена", "cancel")]);
    InlineKeyboardMarkup::new(buttons)
}

pub fn topics_kb(section: &str) -> InlineKeyboardMarkup {
    let mut buttons = vec![];

    let topics = match section {
        "Основы Питона" => vec![
            ("op1", "Вводный урок"),
            ("op2", "Условия и операторы"),
            ("op3", "Цикл for"),
            ("op4", "Цикл while"),
            ("op5", "Практика: циклы"),
            ("op6", "Строки и срезы"),
            ("op7", "Списки")
        ],
        "ЕГЭ 1-27" => {
            let mut t = Vec::new();
            for i in 1..=27 {
                t.push((format!("ege{}", i), format!("Задание {}", i)));
            }
            return InlineKeyboardMarkup::new(
                t.into_iter().map(|(id, title)|
                    vec![InlineKeyboardButton::callback(title, format!("topic|{}|{}", section, id))]
                ).chain(std::iter::once(vec![InlineKeyboardButton::callback("Отмена", "cancel")]))
            );
        },
        _ => vec![]
    };

    for (id, title) in topics {
        buttons.push(vec![InlineKeyboardButton::callback(title, format!("topic|{}|{}", section, id))]);
    }
    buttons.push(vec![InlineKeyboardButton::callback("Отмена", "cancel")]);
    InlineKeyboardMarkup::new(buttons)
}

pub fn admin_kb() -> InlineKeyboardMarkup {
    let buttons = vec![
        vec![InlineKeyboardButton::callback("📋 Дневной отчёт", "admin|daily_full")],
        vec![InlineKeyboardButton::callback("📤 Выслать сейчас", "admin|send_daily_now")],
        vec![InlineKeyboardButton::callback("📊 Полная история", "admin|full_history_manual")],
        vec![InlineKeyboardButton::callback("👤 Выгрузить ученика", "admin|export_user")],
        vec![InlineKeyboardButton::callback("🗑️ Удалить ученика", "admin|delete_user")],
        vec![InlineKeyboardButton::callback("♻️ Сброс базы", "admin|reset_all")],
        vec![InlineKeyboardButton::callback("Отмена", "cancel")],
    ];
    InlineKeyboardMarkup::new(buttons)
}

pub fn get_topic_title(section: &str, topic_id: &str) -> Option<String> {
    if section == "ЕГЭ 1-27" {
        if topic_id.starts_with("ege") {
            let num = topic_id.trim_start_matches("ege");
            return Some(format!("Задание {}", num));
        }
    }

    let topics = match section {
        "Основы Питона" => vec![
            ("op1", "Вводный урок"),
            ("op2", "Условия и операторы"),
            ("op3", "Цикл for"),
            ("op4", "Цикл while"),
            ("op5", "Практика: циклы"),
            ("op6", "Строки и срезы"),
            ("op7", "Списки")
        ],
        _ => vec![]
    };

    topics.into_iter()
        .find(|(id, _)| *id == topic_id)
        .map(|(_, title)| title.to_string())
}