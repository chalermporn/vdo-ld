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
    download, ensure_tools, file_into, human_size, update, vdo_root, verify, ytdlp_path,
    DownloadOpts, VResult,
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

Options:
  --audio          โหลดเสียงอย่างเดียว (mp3)
  --quality N      จำกัดความสูงสูงสุด เช่น 1080, 720 (ไม่ใส่ = สูงสุด)

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

    // แยก flag กับ positional
    let mut opts = DownloadOpts::default();
    let mut pos: Vec<&str> = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--audio" => opts.audio = true,
            "--quality" => opts.max_height = it.next().and_then(|q| parse_quality(q)),
            s if s.starts_with("--quality=") => opts.max_height = parse_quality(&s[10..]),
            s => pos.push(s),
        }
    }

    let url = *pos.first().ok_or("ใส่ URL: vdo-dl <URL> [\"ชื่อเรื่อง\"] [\"หมวด\"]")?;
    let title = pos.get(1).copied().unwrap_or("");
    let category = pos.get(2).copied().unwrap_or("");

    let tools = ensure_tools(&|m| info(m))?;
    info(if opts.audio {
        "กำลังโหลดเสียง (mp3) ..."
    } else {
        "กำลังโหลดคุณภาพสูงสุด (bv*+ba → merge mp4, ไม่ re-encode) ..."
    });

    let cancel = AtomicBool::new(false);
    let file = download(&tools, url, &vdo_root().join("tmp"), &opts, &cancel, &|pct, line| {
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
