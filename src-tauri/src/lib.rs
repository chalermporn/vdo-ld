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
    index: Option<u32>, // ลำดับใน playlist (None = คลิปเดี่ยว) → map ไปแถวลูก (Option B)
    pct: f32,
    text: String,
}

/// item หนึ่งใน playlist โหลด+ย้ายเสร็จ (อัปเดตแถวลูกราย clip)
#[derive(Clone, Serialize)]
struct ItemEvent {
    id: u64,
    index: Option<u32>,
    path: String,
    height: String,
    size: String,
}

#[derive(Clone, Serialize)]
struct DownloadResult {
    id: u64,
    path: String,  // เดี่ยว = path ไฟล์; playlist = path โฟลเดอร์
    count: u32,    // เดี่ยว = 1; playlist = จำนวนไฟล์ที่ได้
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

/// ข้อมูล playlist สำหรับ pre-create แถวลูกใน GUI
#[derive(Clone, Serialize)]
struct PlaylistInfo {
    title: String,
    entries: Vec<String>,
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
fn probe_meta(
    url: String,
    cookies_browser: Option<String>,
    cookies_file: Option<String>,
) -> Result<Meta, String> {
    let (title, thumbnail) = core::probe_meta(&url, &core::Cookies::from(cookies_browser, cookies_file))?;
    Ok(Meta { title, thumbnail })
}

/// ดึงข้อมูล playlist (ชื่อ + รายชื่อคลิป) เพื่อ pre-create แถวลูก (Option B)
#[tauri::command]
fn playlist_probe(
    url: String,
    cookies_browser: Option<String>,
    cookies_file: Option<String>,
) -> Result<PlaylistInfo, String> {
    let (title, entries) =
        core::probe_playlist(&url, &core::Cookies::from(cookies_browser, cookies_file))?;
    Ok(PlaylistInfo { title, entries })
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
    playlist: bool,
    cookies_browser: Option<String>,
    cookies_file: Option<String>,
    wait_for_video: Option<u32>, // รอไลฟ์/พรีเมียร์ที่ตั้งเวลา: ช่วง poll (วินาที); None = ไม่รอ
) -> Result<DownloadResult, String> {
    let cookies = core::Cookies::from(cookies_browser, cookies_file);
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

        // playlist: รู้ subfolder ก่อนเริ่ม เพื่อ file แต่ละ item ได้ทันที (live ไม่รอจบทั้งชุด)
        let subfolder = if playlist {
            if !title.trim().is_empty() {
                title.clone()
            } else {
                core::probe_playlist(&url, &cookies).map(|(t, _)| t).unwrap_or_else(|_| "playlist".into())
            }
        } else {
            String::new()
        };
        // เก็บ path ปลายทางที่ file แล้ว (callback เขียน, นอก loop อ่านไปทำ .m3u)
        let dests: Arc<Mutex<Vec<std::path::PathBuf>>> = Arc::new(Mutex::new(Vec::new()));

        // event callback: progress/status แนบ index; playlist → file ทันทีตอน ItemDone แล้วแจ้งแถวลูก
        let on = {
            let app = app.clone();
            let ffprobe = tools.ffprobe.clone();
            let category = category.clone();
            let subfolder = subfolder.clone();
            let dests = dests.clone();
            let url = url.clone();
            move |ev: core::DlEvent| match ev {
                core::DlEvent::Progress { index, pct } => {
                    let _ = app.emit(
                        "vdo://progress",
                        ProgressEvent { id, index, pct, text: String::new() },
                    );
                }
                core::DlEvent::Status { line, .. } => {
                    let _ = app.emit("vdo://status", StatusEvent { id, text: line.to_string() });
                }
                core::DlEvent::ItemDone { index, path, meta } => {
                    if !playlist {
                        // คลิปเดี่ยว: flip bar เต็ม (filing ทำหลัง download คืน)
                        let _ = app.emit(
                            "vdo://progress",
                            ProgressEvent { id, index, pct: 100.0, text: String::new() },
                        );
                        return;
                    }
                    match core::file_into_dir(&path, &category, &subfolder) {
                        Ok(dest) => {
                            let v = core::verify(&ffprobe, &dest);
                            // index แต่ละ item — title จาก yt-dlp (fallback ชื่อไฟล์), source = URL ทั้งชุด
                            let item_title = if !meta.title.is_empty() {
                                meta.title.clone()
                            } else {
                                dest.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
                            };
                            let _ = core::index_record(&url, index, &meta, &item_title, &category, &dest, &v);
                            let _ = app.emit(
                                "vdo://item",
                                ItemEvent {
                                    id,
                                    index,
                                    path: dest.display().to_string(),
                                    height: v.height,
                                    size: core::human_size(v.size_bytes),
                                },
                            );
                            dests.lock().unwrap().push(dest);
                        }
                        Err(e) => {
                            let _ = app.emit(
                                "vdo://status",
                                StatusEvent { id, text: format!("ย้ายไฟล์ไม่ได้: {}", e) },
                            );
                        }
                    }
                }
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
            playlist,
            cookies,
            wait_for_video,
        };
        let items = core::download(&tools, &url, &core::vdo_root().join("tmp"), &opts, &flag, &on)?;

        let _ = app.emit("vdo://status", StatusEvent { id, text: "กำลัง merge / ตรวจไฟล์…".into() });

        // playlist: file ทำใน callback แล้ว — เหลือเขียน .m3u + คืนสรุป
        if playlist {
            let dests = dests.lock().unwrap();
            if dests.is_empty() {
                return Err("ไม่ได้ไฟล์จาก playlist".into());
            }
            let dir = dests[0].parent().unwrap_or(&dests[0]).to_path_buf();
            let _ = core::write_m3u(&dir, &subfolder, &dests);
            return Ok::<DownloadResult, String>(DownloadResult {
                id,
                path: dir.display().to_string(),
                count: dests.len() as u32,
                width: String::new(),
                height: String::new(),
                vcodec: String::new(),
                acodec: String::new(),
                size: String::new(),
            });
        }

        // วิดีโอเดี่ยว
        let it = &items[0];
        let file = &it.path;
        let v = core::verify(&tools.ffprobe, file);
        let final_path = if title.trim().is_empty() {
            file.clone()
        } else {
            let dest = core::file_into(file, &category, &title)?;
            let _ = core::index_record(&url, it.index, &it.meta, &title, &category, &dest, &v);
            dest
        };

        Ok::<DownloadResult, String>(DownloadResult {
            id,
            path: final_path.display().to_string(),
            count: 1,
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
            playlist_probe,
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
