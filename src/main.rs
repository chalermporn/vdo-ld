// vdo-dl — CLI (บางๆ ครอบ core ใน src/lib.rs)
//
// Usage:
//   vdo-dl <URL>                      โหลดคุณภาพสูงสุดไปพักที่ ~/VDO/tmp/ (พิมพ์ path ออกมา)
//   vdo-dl <URL> "ชื่อเรื่อง"          โหลด + ตั้งชื่อ + วางที่ ~/VDO/ยังไม่จัดหมวด/
//   vdo-dl <URL> "ชื่อเรื่อง" "หมวด"   โหลด + ตั้งชื่อ + วางที่ ~/VDO/<หมวด>/
//   vdo-dl -F <URL>                   ดูตาราง format ที่มี (ไม่โหลด)
//   vdo-dl update                     อัปเดต yt-dlp (+ ffmpeg บน Windows) ที่ bundle ไว้

use std::env;
use std::process::Command;
use vdo_dl::{
    download, ensure_tools, file_into, human_size, update, vdo_root, verify, ytdlp_path, VResult,
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
  vdo-dl <URL>                      โหลด → พักที่ ~/VDO/tmp/ (พิมพ์ path)
  vdo-dl <URL> \"ชื่อเรื่อง\"          โหลด + ตั้งชื่อ → ~/VDO/ยังไม่จัดหมวด/
  vdo-dl <URL> \"ชื่อเรื่อง\" \"หมวด\"   โหลด + ตั้งชื่อ → ~/VDO/<หมวด>/
  vdo-dl -F <URL>                   ดูตาราง format ที่มี (ไม่โหลด)
  vdo-dl update                     อัปเดต yt-dlp (+ ffmpeg บน Windows) ที่ bundle ไว้

ครั้งแรกที่รัน ถ้าเครื่องไม่มี yt-dlp/ffmpeg จะโหลดมาเก็บเองที่ <data>/vdo-dl/bin/
(ไม่ต้องลงเพิ่มเอง). Env: VDO_ROOT, VDO_BIN, NO_COLOR";

fn run() -> VResult<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(|s| s.as_str()) {
        None | Some("-h") | Some("--help") => {
            println!("{}", USAGE);
            return Ok(());
        }
        Some("update") => return update(&|m| info(m)),
        Some("-F") => {
            let url = args.get(1).ok_or("ใส่ URL ด้วย: vdo-dl -F <URL>")?;
            let yt = ytdlp_path(&|m| info(m))?;
            let status = Command::new(&yt)
                .arg("-F")
                .arg(url)
                .status()
                .map_err(|e| format!("รัน yt-dlp ไม่ได้: {}", e))?;
            std::process::exit(status.code().unwrap_or(1));
        }
        _ => {}
    }

    let url = &args[0];
    let title = args.get(1).map(|s| s.as_str()).unwrap_or("");
    let category = args.get(2).map(|s| s.as_str()).unwrap_or("");

    let tools = ensure_tools(&|m| info(m))?;
    info("กำลังโหลดคุณภาพสูงสุด (bv*+ba → merge mp4, ไม่ re-encode) ...");

    let file = download(&tools, url, &vdo_root().join("tmp"), &|pct, line| {
        if pct >= 0.0 {
            eprint!("\r{}", paint("36", &format!("  {:>5.1}%", pct)));
        } else {
            eprintln!("{}", line);
        }
    })?;
    eprintln!();

    let v = verify(&tools.ffprobe, &file);
    ok(&format!(
        "ได้ไฟล์: {}x{} {}/{}  ({})",
        v.width, v.height, v.vcodec, v.acodec, human_size(v.size_bytes)
    ));

    if title.is_empty() {
        ok(&format!("พักไว้ที่: {}", file.display()));
        println!("{}", file.display());
        return Ok(());
    }

    let dest = file_into(&file, category, title)?;
    ok(&format!("ย้ายเข้า: {}", dest.display()));
    println!("{}", dest.display());
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        die(&e);
    }
}
