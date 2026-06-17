// vdo-dl Tauri app — ครอบ core (crate vdo-dl) แล้วเปิดเป็น command + event ให้ frontend เรียก
//
// frontend (ui/index.html) เรียกผ่าน window.__TAURI__.core.invoke(...) และฟัง progress ด้วย
// window.__TAURI__.event.listen("vdo://progress", ...). ทุก event ใช้ชื่อขึ้นต้น "vdo://".

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use vdo_dl as core;

#[derive(Clone, Serialize)]
struct StatusEvent {
    text: String,
}

#[derive(Clone, Serialize)]
struct ProgressEvent {
    pct: f32,
    text: String,
}

#[derive(Clone, Serialize)]
struct DownloadResult {
    path: String,
    width: String,
    height: String,
    vcodec: String,
    acodec: String,
    size: String,
}

/// หมวดที่มีอยู่ใน ~/VDO/ (ทำ dropdown)
#[tauri::command]
fn categories() -> Vec<String> {
    core::list_categories()
}

/// path ของ ~/VDO (โชว์ใน status bar)
#[tauri::command]
fn vdo_root() -> String {
    core::vdo_root().display().to_string()
}

/// โหลดวิดีโอ → verify → จัดหมวด. ส่ง event ระหว่างทาง, คืนผลลัพธ์ตอนจบ.
#[tauri::command]
fn download_video(
    app: AppHandle,
    url: String,
    title: String,
    category: String,
) -> Result<DownloadResult, String> {
    let status = {
        let app = app.clone();
        move |m: &str| {
            let _ = app.emit("vdo://status", StatusEvent { text: m.to_string() });
        }
    };

    let tools = core::ensure_tools(&status)?;
    status("กำลังโหลดคุณภาพสูงสุด…");

    let on_progress = {
        let app = app.clone();
        move |pct: f32, line: &str| {
            let _ = app.emit("vdo://progress", ProgressEvent { pct, text: line.to_string() });
        }
    };

    let file = core::download(&tools, &url, &core::vdo_root().join("tmp"), &on_progress)?;

    let _ = app.emit("vdo://status", StatusEvent { text: "กำลัง merge / ตรวจไฟล์…".into() });
    let v = core::verify(&tools.ffprobe, &file);

    // ไม่มีชื่อ → ปล่อยไว้ที่ tmp
    let final_path = if title.trim().is_empty() {
        file.clone()
    } else {
        core::file_into(&file, &category, &title)?
    };

    Ok(DownloadResult {
        path: final_path.display().to_string(),
        width: v.width,
        height: v.height,
        vcodec: v.vcodec,
        acodec: v.acodec,
        size: core::human_size(v.size_bytes),
    })
}

/// อัปเดต yt-dlp (+ ffmpeg บน Windows) ที่ bundle ไว้
#[tauri::command]
fn update_tools(app: AppHandle) -> Result<(), String> {
    let status = move |m: &str| {
        let _ = app.emit("vdo://status", StatusEvent { text: m.to_string() });
    };
    core::update(&status)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            categories,
            vdo_root,
            download_video,
            update_tools
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
