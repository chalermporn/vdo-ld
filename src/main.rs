// vdo-dl — CLI (บางๆ ครอบ core ใน src/lib.rs)
//
// Usage:
//   vdo-dl [--audio] [--quality N] <URL> ["ชื่อเรื่อง"] ["หมวด"]
//   vdo-dl -F <URL>        ดูตาราง format ที่มี (ไม่โหลด)
//   vdo-dl update          อัปเดต yt-dlp (+ ffmpeg บน Windows) ที่ bundle ไว้

use std::env;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use vdo_dl::{
    backfill_index, download, ensure_tools, file_into, file_into_dir, human_size, index_record,
    prepend_tool_path, probe_playlist, search_index, update, vdo_root, verify, write_m3u,
    ytdlp_path, AudioFmt, Container, Cookies, DlEvent, DownloadOpts, VResult,
};

fn color_on() -> bool {
    env::var_os("NO_COLOR").is_none()
}
fn paint(code: &str, msg: &str) -> String {
    if color_on() {
        format!("\x1b[{}m{}\x1b[0m", code, msg)
    } else {
        msg.to_string()
    }
}
fn info(msg: &str) {
    eprintln!("{}", paint("36", &format!("▸ {}", msg)));
}
fn ok(msg: &str) {
    eprintln!("{}", paint("32", &format!("✓ {}", msg)));
}
fn die(msg: &str) -> ! {
    eprintln!("{}", paint("31", &format!("✗ {}", msg)));
    std::process::exit(1);
}

const USAGE: &str = "\
vdo-dl — โหลดวิดีโอคุณภาพสูงสุด แล้วจัดเข้า ~/VDO/

Usage:
  vdo-dl [--audio] [--quality N] <URL> [\"ชื่อเรื่อง\"] [\"หมวด\"]
  vdo-dl -F <URL>        ดูตาราง format ที่มี (ไม่โหลด)
  vdo-dl update          อัปเดต yt-dlp (+ ffmpeg บน Windows) ที่ bundle ไว้
  vdo-dl search [คำค้น]  ค้นประวัติที่เคยโหลด (title/ผู้ลง/หมวด/URL/แหล่ง); ไม่ใส่คำ = ล่าสุด
  vdo-dl backfill        สแกนไฟล์ใน ~/VDO/ ที่ยังไม่อยู่ใน index แล้วเพิ่มเข้า (ของเก่าไม่มี URL)

Options:
  --audio              โหลดเสียงอย่างเดียว
  --quality N          จำกัดความสูงสูงสุด เช่น 1080, 720 (ไม่ใส่ = สูงสุด)
  --mkv                วิดีโอ: merge เป็น .mkv (default .mp4)
  --audio-format FMT   เสียง: mp3 | m4a | ogg (default mp3)
  --audio-quality N    เสียง: 0(ดีสุด)..10 (ไม่ใส่ = ค่า default)
  --subs[=LANGS]       วิดีโอ: ดาวน์โหลด+ฝังคำบรรยาย (ไม่ระบุ = en,th)
  --playlist           โหลดทั้ง playlist → ~/VDO/<หมวด>/<ชื่อ/playlist>/ + .m3u
  --wait-for-video[=N] รอไลฟ์/พรีเมียร์ที่ตั้งเวลาให้เริ่มก่อน (poll ทุก N วิ, default 60)
  --cookies-from-browser B   ใช้คุกกี้จากเบราว์เซอร์ (chrome|safari|firefox|edge|brave|…)
                             สำหรับเนื้อหาที่ต้องล็อกอิน (เช่น คอร์สที่ enroll, ไม่มี DRM)
  --cookies FILE             ใช้คุกกี้จากไฟล์ cookies.txt (Netscape)

ครั้งแรกที่รัน ถ้าเครื่องไม่มี yt-dlp/ffmpeg จะโหลดมาเก็บเองที่ <data>/vdo-dl/bin/.
Env: VDO_ROOT, VDO_BIN, NO_COLOR";

fn parse_quality(q: &str) -> Option<u32> {
    match q.trim().to_lowercase().as_str() {
        "best" | "max" | "สูงสุด" | "" => None,
        s => s.trim_end_matches('p').parse::<u32>().ok(),
    }
}

fn run() -> VResult<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(|s| s.as_str()) {
        None | Some("-h") | Some("--help") => {
            println!("{}", USAGE);
            return Ok(());
        }
        Some("update") => return update(&|m| info(m)),
        Some("search") => {
            let q = args.get(1).map(|s| s.as_str()).unwrap_or("");
            print!("{}", search_index(q)?);
            return Ok(());
        }
        Some("backfill") => {
            let tools = ensure_tools(&|m| info(m))?;
            let n = backfill_index(&tools.ffprobe, &|m| info(m))?;
            ok(&format!("เพิ่มเข้า index {} รายการ", n));
            return Ok(());
        }
        Some("-F") => {
            let url = args.get(1).ok_or("ใส่ URL ด้วย: vdo-dl -F <URL>")?;
            let yt = ytdlp_path(&|m| info(m))?;
            let mut cmd = Command::new(&yt);
            prepend_tool_path(&mut cmd);
            let status = cmd
                .arg("-F")
                .arg(url)
                .status()
                .map_err(|e| format!("รัน yt-dlp ไม่ได้: {}", e))?;
            std::process::exit(status.code().unwrap_or(1));
        }
        _ => {}
    }

    // แยก flag กับ positional
    let mut opts = DownloadOpts::default();
    let mut pos: Vec<&str> = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--audio" => opts.audio = true,
            "--quality" => opts.max_height = it.next().and_then(|q| parse_quality(q)),
            s if s.starts_with("--quality=") => opts.max_height = parse_quality(&s[10..]),
            "--mkv" => opts.container = Container::Mkv,
            "--audio-format" => {
                if let Some(f) = it.next() {
                    opts.audio_fmt = AudioFmt::parse(f);
                }
            }
            s if s.starts_with("--audio-format=") => opts.audio_fmt = AudioFmt::parse(&s[15..]),
            "--audio-quality" => opts.audio_quality = it.next().and_then(|q| q.trim().parse().ok()),
            s if s.starts_with("--audio-quality=") => opts.audio_quality = s[16..].trim().parse().ok(),
            "--subs" => {
                opts.subs = true;
                opts.sub_langs = "en,th".into();
            }
            s if s.starts_with("--subs=") => {
                opts.subs = true;
                opts.sub_langs = s[7..].to_string();
            }
            "--playlist" => opts.playlist = true,
            // รอไลฟ์/พรีเมียร์ที่ตั้งเวลา: bare = poll ทุก 60 วิ, =N = ทุก N วิ
            "--wait-for-video" => opts.wait_for_video = Some(60),
            s if s.starts_with("--wait-for-video=") => {
                opts.wait_for_video = s[17..].trim().parse().ok().or(Some(60));
            }
            "--cookies-from-browser" => {
                if let Some(b) = it.next() {
                    opts.cookies = Cookies::Browser(b.clone());
                }
            }
            s if s.starts_with("--cookies-from-browser=") => {
                opts.cookies = Cookies::Browser(s[23..].to_string());
            }
            "--cookies" => {
                if let Some(f) = it.next() {
                    opts.cookies = Cookies::File(f.into());
                }
            }
            s if s.starts_with("--cookies=") => {
                opts.cookies = Cookies::File(s[10..].into());
            }
            s => pos.push(s),
        }
    }

    let url = *pos.first().ok_or("ใส่ URL: vdo-dl <URL> [\"ชื่อเรื่อง\"] [\"หมวด\"]")?;
    let title = pos.get(1).copied().unwrap_or("");
    let category = pos.get(2).copied().unwrap_or("");

    let tools = ensure_tools(&|m| info(m))?;
    if opts.audio {
        info(&format!("กำลังโหลดเสียง ({}) ...", opts.audio_fmt.as_str()));
    } else {
        info(&format!(
            "กำลังโหลดคุณภาพสูงสุด (bv*+ba → merge {}, ไม่ re-encode){} ...",
            opts.container.as_str(),
            if opts.subs { " + ซับ" } else { "" }
        ));
    }

    let cancel = AtomicBool::new(false);
    let on = |ev: DlEvent| match ev {
        DlEvent::Progress { index, pct } if pct >= 0.0 => match index {
            Some(i) => eprint!("\r{}", paint("36", &format!("  #{:<2} {:>5.1}%", i, pct))),
            None => eprint!("\r{}", paint("36", &format!("  {:>5.1}%", pct))),
        },
        DlEvent::Progress { .. } => {}
        DlEvent::Status { line, .. } => eprintln!("{}", line),
        DlEvent::ItemDone { index: Some(i), path, .. } => {
            eprintln!();
            ok(&format!("#{} เสร็จ: {}", i, path.display()));
        }
        DlEvent::ItemDone { .. } => {}
    };
    let items = download(&tools, url, &vdo_root().join("tmp"), &opts, &cancel, &on)?;
    eprintln!();

    // playlist: ย้ายแต่ละไฟล์เข้า subfolder (คงชื่อมีเลขลำดับ) + เขียน .m3u
    if opts.playlist {
        let subfolder = if !title.is_empty() {
            title.to_string()
        } else {
            probe_playlist(url, &opts.cookies).map(|(t, _)| t).unwrap_or_else(|_| "playlist".into())
        };
        let mut dests = vec![];
        for it in &items {
            let dest = file_into_dir(&it.path, category, &subfolder)?;
            // index แต่ละ item — title มาจาก yt-dlp (fallback ชื่อไฟล์), source = URL ทั้งชุด
            let v = verify(&tools.ffprobe, &dest);
            let item_title = if !it.meta.title.is_empty() {
                it.meta.title.clone()
            } else {
                dest.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
            };
            let _ = index_record(url, it.index, &it.meta, &item_title, category, &dest, &v);
            dests.push(dest);
        }
        let dir = dests[0].parent().unwrap_or(&dests[0]).to_path_buf();
        ok(&format!("playlist {} ไฟล์ → {}", dests.len(), dir.display()));
        if let Ok(m3u) = write_m3u(&dir, &subfolder, &dests) {
            ok(&format!("เพลย์ลิสต์: {}", m3u.display()));
        }
        for d in &dests {
            println!("{}", d.display());
        }
        return Ok(());
    }

    // วิดีโอเดี่ยว
    let it = &items[0];
    let file = &it.path;
    let v = verify(&tools.ffprobe, file);
    ok(&format!(
        "ได้ไฟล์: {}x{} {}/{}  ({})",
        v.width, v.height, v.vcodec, v.acodec, human_size(v.size_bytes)
    ));

    if title.is_empty() {
        ok(&format!("พักไว้ที่: {}", file.display()));
        println!("{}", file.display());
        return Ok(());
    }

    let dest = file_into(file, category, title)?;
    ok(&format!("ย้ายเข้า: {}", dest.display()));
    let _ = index_record(url, it.index, &it.meta, title, category, &dest, &v);
    println!("{}", dest.display());
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        die(&e);
    }
}
