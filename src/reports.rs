use std::io::{Cursor, Write};
use std::path::Path;
use std::collections::HashMap;

use teloxide::{prelude::*, types::InputFile};
use teloxide::net::Download;
use sqlx::{SqlitePool, Row};
use rust_xlsxwriter::{Workbook, Format, Color as XlsxColor};
use plotters::prelude::*;
use plotters::style::Color;
use zip::write::FileOptions;
use walkdir::WalkDir;
use chrono::Utc;

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
}

pub async fn save_text_to_disk(
    text: &str,
    base_dir: &str,
    user_id: i64,
    section: &str,
    topic_id: &str
) -> anyhow::Result<()> {
    let safe_sec = slugify(section);
    let safe_topic = slugify(topic_id);
    let dir_path = format!("{}/{}/{}_{}", base_dir, user_id, safe_sec, safe_topic);

    tokio::fs::create_dir_all(&dir_path).await?;
    let filename = format!("{}.txt", Utc::now().format("%Y%m%d_%H%M%S"));
    let full_path = format!("{}/{}", dir_path, filename);

    tokio::fs::write(full_path, text).await?;
    Ok(())
}

pub async fn save_file_to_disk(
    bot: &Bot,
    file_id: &str,
    base_dir: &str,
    user_id: i64,
    section: &str,
    topic_id: &str
) -> anyhow::Result<()> {
    let safe_sec = slugify(section);
    let safe_topic = slugify(topic_id);
    let dir_path = format!("{}/{}/{}_{}", base_dir, user_id, safe_sec, safe_topic);

    tokio::fs::create_dir_all(&dir_path).await?;

    let file_info = bot.get_file(file_id.to_string()).await?;
    let extension = file_info.path.split('.').last().unwrap_or("jpg");
    let filename = format!("file_{}.{}", Utc::now().format("%Y%m%d_%H%M%S_%f"), extension);
    let full_path = format!("{}/{}", dir_path, filename);

    let mut dst = tokio::fs::File::create(full_path).await?;
    bot.download_file(&file_info.path, &mut dst).await?;

    Ok(())
}

pub async fn generate_daily_report(pool: &SqlitePool, date: &str) -> anyhow::Result<Vec<u8>> {
    let mut workbook = Workbook::new();

    let sheet_raw = workbook.add_worksheet().set_name("raw_submissions")?;
    let header_format = Format::new().set_bold().set_background_color(XlsxColor::RGB(0xA7F3D0));

    sheet_raw.write_row_with_format(0, 0, [
        "User ID", "Username", "Name", "Type", "Section", "Topic", "Summary", "Date", "TS"
    ], &header_format)?;

    let rows = sqlx::query(
        "SELECT s.user_id, u.username, u.first_name, s.type, s.section, s.topic_title, s.content_summary, s.date, s.ts
         FROM submissions s LEFT JOIN users u ON s.user_id = u.id WHERE s.date = ?"
    )
        .bind(date)
        .fetch_all(pool)
        .await?;

    for (i, row) in rows.iter().enumerate() {
        let r = (i + 1) as u32;
        let uid: i64 = row.get("user_id");
        sheet_raw.write(r, 0, uid)?;
        sheet_raw.write(r, 1, row.get::<Option<String>, _>("username").unwrap_or_default())?;
        sheet_raw.write(r, 2, row.get::<Option<String>, _>("first_name").unwrap_or_default())?;
        sheet_raw.write(r, 3, row.get::<String, _>("type"))?;
        sheet_raw.write(r, 4, row.get::<String, _>("section"))?;
        sheet_raw.write(r, 5, row.get::<String, _>("topic_title"))?;
        sheet_raw.write(r, 6, row.get::<String, _>("content_summary"))?;
        sheet_raw.write(r, 7, row.get::<String, _>("date"))?;
        sheet_raw.write(r, 8, row.get::<String, _>("ts"))?;
    }
    sheet_raw.autofit();

    let sheet_sum = workbook.add_worksheet().set_name("daily_summary")?;
    sheet_sum.write_row_with_format(0, 0, [
        "User ID", "Name", "DZ Submitted", "Conspect Submitted", "Miss Reason", "Task Flag"
    ], &header_format)?;

    let users = sqlx::query("SELECT id, username, first_name FROM users ORDER BY id").fetch_all(pool).await?;

    for (i, user_row) in users.iter().enumerate() {
        let r = (i + 1) as u32;
        let uid: i64 = user_row.get("id");
        let uname: Option<String> = user_row.get("username");
        let fname: String = user_row.get("first_name");
        let display_name = uname.unwrap_or(fname);

        let dz_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM submissions WHERE user_id = ? AND date = ? AND type = 'dz'")
            .bind(uid).bind(date).fetch_one(pool).await.unwrap_or(0);

        let conspect_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM submissions WHERE user_id = ? AND date = ? AND type = 'conspect'")
            .bind(uid).bind(date).fetch_one(pool).await.unwrap_or(0);

        let reason: String = sqlx::query_scalar("SELECT reason FROM miss_reasons WHERE user_id = ? AND date = ?")
            .bind(uid).bind(date).fetch_one(pool).await.unwrap_or_default();

        let task_flag: String = sqlx::query_scalar(
            "SELECT topic_title FROM submissions WHERE user_id = ? AND date = ? ORDER BY ts DESC LIMIT 1"
        ).bind(uid).bind(date).fetch_one(pool).await.unwrap_or_default();

        sheet_sum.write(r, 0, uid)?;
        sheet_sum.write(r, 1, display_name)?;
        sheet_sum.write(r, 2, dz_count)?;
        sheet_sum.write(r, 3, conspect_count)?;
        sheet_sum.write(r, 4, reason)?;
        sheet_sum.write(r, 5, task_flag)?;
    }
    sheet_sum.autofit();

    Ok(workbook.save_to_buffer()?)
}

fn create_miss_chart(data: &HashMap<String, i64>) -> anyhow::Result<Vec<u8>> {
    let mut buffer = vec![0; 0];
    {
        let root = BitMapBackend::with_buffer(&mut buffer, (800, 600)).into_drawing_area();
        root.fill(&WHITE)?;

        let mut sorted_data: Vec<(&String, &i64)> = data.iter().collect();
        sorted_data.sort_by(|a, b| b.1.cmp(a.1));
        let top_data = sorted_data.into_iter().take(15).collect::<Vec<_>>();

        if top_data.is_empty() { return Ok(vec![]); }

        let max_val = *top_data[0].1 as i32 + 1;

        let mut chart = ChartBuilder::on(&root)
            .caption("Топ пропусков", ("sans-serif", 30))
            .margin(10)
            .x_label_area_size(40)
            .y_label_area_size(40)
            .build_cartesian_2d(
                (0..top_data.len()).into_segmented(),
                0..max_val
            )?;

        chart.configure_mesh()
            .x_labels(top_data.len())
            .x_label_formatter(&|x| match x {
                SegmentValue::CenterOf(i) | SegmentValue::Exact(i) => {
                    if *i < top_data.len() { top_data[*i].0.clone() } else { "".to_string() }
                },
                _ => "".to_string()
            })
            .draw()?;

        chart.draw_series(
            top_data.iter().enumerate().map(|(x, (_name, val))| {
                let v = **val as i32;
                Rectangle::new(
                    [(x.into(), 0), (x.into(), v)],
                    RED.filled()
                )
            })
        )?;
        root.present()?;
    }
    Ok(buffer)
}

fn create_top_students_chart(data: &HashMap<String, i64>, title: &str) -> anyhow::Result<Vec<u8>> {
    let mut buffer = vec![0; 0];
    {
        let root = BitMapBackend::with_buffer(&mut buffer, (800, 600)).into_drawing_area();
        root.fill(&WHITE)?;

        let mut sorted_data: Vec<(&String, &i64)> = data.iter().collect();
        sorted_data.sort_by(|a, b| b.1.cmp(a.1));
        let top_data = sorted_data.into_iter().take(15).collect::<Vec<_>>();

        if top_data.is_empty() { return Ok(vec![]); }
        let max_val = *top_data[0].1 as i32 + 1;

        let mut chart = ChartBuilder::on(&root)
            .caption(title, ("sans-serif", 30))
            .margin(10)
            .x_label_area_size(40)
            .y_label_area_size(40)
            .build_cartesian_2d(
                (0..top_data.len()).into_segmented(),
                0..max_val
            )?;

        chart.configure_mesh()
            .x_labels(top_data.len())
            .x_label_formatter(&|x| match x {
                SegmentValue::CenterOf(i) | SegmentValue::Exact(i) => {
                    if *i < top_data.len() { top_data[*i].0.clone() } else { "".to_string() }
                },
                _ => "".to_string()
            })
            .draw()?;

        chart.draw_series(
            top_data.iter().enumerate().map(|(x, (_name, val))| {
                let v = **val as i32;
                Rectangle::new(
                    [(x.into(), 0), (x.into(), v)],
                    BLUE.filled()
                )
            })
        )?;
        root.present()?;
    }
    Ok(buffer)
}

pub async fn generate_full_history_package(pool: &SqlitePool) -> anyhow::Result<Vec<InputFile>> {
    let mut files = Vec::new();

    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet().set_name("ALL_HISTORY")?;
    let header_format = Format::new().set_bold().set_background_color(XlsxColor::RGB(0xA7F3D0));

    sheet.write_row_with_format(0, 0, [
        "Date", "User ID", "Name", "Type", "Topic", "Summary"
    ], &header_format)?;

    let rows = sqlx::query(
        "SELECT s.date, s.user_id, u.first_name, s.type, s.topic_title, s.content_summary
         FROM submissions s LEFT JOIN users u ON s.user_id = u.id ORDER BY s.date DESC"
    ).fetch_all(pool).await?;

    let mut dz_stats: HashMap<String, i64> = HashMap::new();
    let mut conspect_stats: HashMap<String, i64> = HashMap::new();
    let mut miss_stats: HashMap<String, i64> = HashMap::new();

    for (i, row) in rows.iter().enumerate() {
        let r = (i + 1) as u32;
        let date: String = row.get("date");
        let uid: i64 = row.get("user_id");
        let name: String = row.get("first_name");
        let type_: String = row.get("type");

        sheet.write(r, 0, &date)?;
        sheet.write(r, 1, uid)?;
        sheet.write(r, 2, &name)?;
        sheet.write(r, 3, &type_)?;
        sheet.write(r, 4, row.get::<String, _>("topic_title"))?;
        sheet.write(r, 5, row.get::<String, _>("content_summary"))?;

        if type_ == "dz" {
            *dz_stats.entry(name.clone()).or_insert(0) += 1;
        } else if type_ == "conspect" {
            *conspect_stats.entry(name.clone()).or_insert(0) += 1;
        }
    }

    let misses = sqlx::query("SELECT u.first_name FROM miss_reasons m JOIN users u ON m.user_id = u.id")
        .fetch_all(pool).await?;
    for row in misses {
        let name: String = row.get("first_name");
        *miss_stats.entry(name).or_insert(0) += 1;
    }

    files.push(InputFile::memory(workbook.save_to_buffer()?).file_name("history.xlsx"));

    if let Ok(png) = create_top_students_chart(&dz_stats, "Топ по ДЗ") {
        if !png.is_empty() { files.push(InputFile::memory(png).file_name("top_dz.png")); }
    }
    if let Ok(png) = create_top_students_chart(&conspect_stats, "Топ по конспектам") {
        if !png.is_empty() { files.push(InputFile::memory(png).file_name("top_conspect.png")); }
    }
    if let Ok(png) = create_miss_chart(&miss_stats) {
        if !png.is_empty() { files.push(InputFile::memory(png).file_name("misses.png")); }
    }

    Ok(files)
}

pub async fn archive_user_conspects(base_dir: &str, user_id: i64) -> anyhow::Result<Vec<u8>> {
    let user_path = format!("{}/{}", base_dir, user_id);
    let path = Path::new(&user_path);

    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut buf = Vec::new();
    let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let walker = WalkDir::new(path).into_iter();
    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            let name = path.strip_prefix(&user_path).unwrap().to_str().unwrap();
            zip.start_file(name, options)?;
            let content = std::fs::read(path)?;
            zip.write_all(&content)?;
        }
    }
    zip.finish()?;
    drop(zip);

    Ok(buf)
}

pub async fn export_user_data(pool: &SqlitePool, base_dir: &str, identifier: &str) -> anyhow::Result<(Vec<u8>, Option<Vec<u8>>)> {
    let user_opt = if let Ok(id) = identifier.parse::<i64>() {
        sqlx::query("SELECT id, username FROM users WHERE id = ?").bind(id).fetch_optional(pool).await?
    } else {
        sqlx::query("SELECT id, username FROM users WHERE username = ?").bind(identifier).fetch_optional(pool).await?
    };

    let (uid, _) = match user_opt {
        Some(row) => (row.get::<i64, _>("id"), row.get::<Option<String>, _>("username").unwrap_or_default()),
        None => return Err(anyhow::anyhow!("User not found")),
    };

    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_row(0, 0, ["Type", "Section", "Topic", "Summary", "Date"])?;

    let rows = sqlx::query("SELECT type, section, topic_title, content_summary, date FROM submissions WHERE user_id = ?")
        .bind(uid)
        .fetch_all(pool)
        .await?;

    for (i, row) in rows.iter().enumerate() {
        let r = (i + 1) as u32;
        sheet.write(r, 0, row.get::<String, _>("type"))?;
        sheet.write(r, 1, row.get::<String, _>("section"))?;
        sheet.write(r, 2, row.get::<String, _>("topic_title"))?;
        sheet.write(r, 3, row.get::<String, _>("content_summary"))?;
        sheet.write(r, 4, row.get::<String, _>("date"))?;
    }

    let excel_buf = workbook.save_to_buffer()?;

    let zip_buf = archive_user_conspects(base_dir, uid).await?;
    let zip_opt = if zip_buf.is_empty() { None } else { Some(zip_buf) };

    Ok((excel_buf, zip_opt))
}