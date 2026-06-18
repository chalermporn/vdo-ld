// vdo-dl Tauri app — ครอบ core (crate vdo-dl) เปิดเป็น command + event ให้ frontend เรียก
//
// frontend (ui/index.html) เรียกผ่าน window.__TAURI__.core.invoke(...) และฟัง event ด้วย
// window.__TAURI__.event.listen("vdo://progress"|"vdo://status", ...). event ทุกตัวแนบ `id`
// เพื่อให้ map กลับไป row ที่ถูกต้อง (รองรับโหลดพร้อมกันหลายอัน).

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use vdo_dl as core;

/// เก็บ cancel flag ต่อ download id (ให้ cancel_download ยกเลิกได้)
#[derive(Default)]
struct Jobs(Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>);

#[derive(Clone, Serialize)]
struct StatusEvent {
    id: u64,
    text: String,
}

#[derive(Clone, Serialize)]
struct ProgressEvent {
    id: u64,
    pct: f32,
    text: String,
}

#[derive(Clone, Serialize)]
struct DownloadResult {
    id: u64,
    path: String,
    width: String,
    height: String,
    vcodec: String,
    acodec: String,
    size: String,
}

#[derive(Clone, Serialize)]
struct Meta {
    title: String,
    thumbnail: String,
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

/// ดึงชื่อเรื่อง + thumbnail จาก URL (เติมให้อัตโนมัติ)
#[tauri::command]
fn probe_meta(url: String) -> Result<Meta, String> {
    let (title, thumbnail) = core::probe_meta(&url)?;
    Ok(Meta { title, thumbnail })
}

/// อ่าน URL จาก clipboard (ปุ่ม "วางลิงก์")
#[tauri::command]
fn clipboard() -> String {
    core::read_clipboard()
}

/// เปิดโฟลเดอร์ที่มีไฟล์นั้นใน file manager
#[tauri::command]
fn reveal(path: String) -> Result<(), String> {
    core::reveal_path(&path)
}

/// ลบไฟล์ในดิสก์ (ใต้ ~/VDO)
#[tauri::command]
fn delete_file(path: String) -> Result<(), String> {
    core::delete_file(&path)
}

/// ยกเลิกการโหลดของ id นั้น
#[tauri::command]
fn cancel_download(jobs: State<'_, Jobs>, id: u64) {
    if let Some(flag) = jobs.0.lock().unwrap().get(&id) {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// โหลดวิดีโอ/เสียง → verify → จัดหมวด. ส่ง event (แนบ id) ระหว่างทาง, คืนผลตอนจบ.
#[tauri::command]
async fn download_video(
    app: AppHandle,
    jobs: State<'_, Jobs>,
    id: u64,
    url: String,
    title: String,
    category: String,
    audio: bool,
    max_height: Option<u32>,
    container: String,
    audio_fmt: String,
    audio_quality: Option<u8>,
    subs: bool,
    sub_langs: String,
) -> Result<DownloadResult, String> {
    let map = jobs.0.clone(); // clone Arc ก่อน await (ไม่ถือ State ข้าม await)
    let flag = Arc::new(AtomicBool::new(false));
    map.lock().unwrap().insert(id, flag.clone());

    let result = tauri::async_runtime::spawn_blocking(move || {
        let status = {
            let app = app.clone();
            move |m: &str| {
                let _ = app.emit("vdo://status", StatusEvent { id, text: m.to_string() });
            }
        };

        let tools = core::ensure_tools(&status)?;
        status("กำลังโหลด…");

        let on = {
            let app = app.clone();
            move |pct: f32, line: &str| {
                let _ = app.emit("vdo://progress", ProgressEvent { id, pct, text: line.to_string() });
            }
        };
        let opts = core::DownloadOpts {
            audio,
            max_height,
            container: core::Container::parse(&container),
            audio_fmt: core::AudioFmt::parse(&audio_fmt),
            audio_quality,
            subs,
            sub_langs,
        };
        let file = core::download(&tools, &url, &core::vdo_root().join("tmp"), &opts, &flag, &on)?;

        let _ = app.emit("vdo://status", StatusEvent { id, text: "กำลัง merge / ตรวจไฟล์…".into() });
        let v = core::verify(&tools.ffprobe, &file);

        let final_path = if title.trim().is_empty() {
            file.clone()
        } else {
            core::file_into(&file, &category, &title)?
        };

        Ok::<DownloadResult, String>(DownloadResult {
            id,
            path: final_path.display().to_string(),
            width: v.width,
            height: v.height,
            vcodec: v.vcodec,
            acodec: v.acodec,
            size: core::human_size(v.size_bytes),
        })
    })
    .await
    .map_err(|e| format!("งานโหลดพัง: {}", e))?;

    map.lock().unwrap().remove(&id);
    result
}

/// อัปเดต yt-dlp (+ ffmpeg บน Windows) ที่ bundle ไว้
#[tauri::command]
async fn update_tools(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let status = move |m: &str| {
            let _ = app.emit("vdo://status", StatusEvent { id: 0, text: m.to_string() });
        };
        core::update(&status)
    })
    .await
    .map_err(|e| format!("งานอัปเดตพัง: {}", e))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Jobs::default())
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
            probe_meta,
            clipboard,
            reveal,
            delete_file,
            cancel_download,
            download_video,
            update_tools
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
