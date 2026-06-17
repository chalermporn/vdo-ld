// vdo-dl core — logic ที่ใช้ร่วมกันระหว่าง CLI (src/main.rs) และ Tauri app (src-tauri/)
//
// ทุกฟังก์ชันคืน Result<_, String> (ไม่มี die/exit) เพื่อให้ GUI จัดการ error ได้โดยไม่ crash.
// สถานะ/ความคืบหน้าส่งออกผ่าน callback — CLI พิมพ์ลง stderr, Tauri แปลงเป็น event.
//
// zero-dependency: ห่อ yt-dlp + ffmpeg, โหลดผ่าน curl, แตก zip ด้วย tar ที่มากับ OS.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub type VResult<T> = Result<T, String>;

/// ตัวเลือกการโหลด: เสียงอย่างเดียวไหม + จำกัดความสูงสูงสุด (None = สูงสุด)
#[derive(Clone, Copy, Default)]
pub struct DownloadOpts {
    pub audio: bool,
    pub max_height: Option<u32>,
}

/// callback รับข้อความสถานะ (เช่น "โหลด yt-dlp ครั้งแรก…")
pub type Log<'a> = dyn Fn(&str) + Sync + 'a;
/// callback รับความคืบหน้าโหลด: (percent 0..100, ข้อความดิบ). percent < 0 = บรรทัดสถานะเฉย ๆ
pub type Progress<'a> = dyn Fn(f32, &str) + Sync + 'a;

pub struct Tools {
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

#[derive(Clone, Debug)]
pub struct VideoInfo {
    pub width: String,
    pub height: String,
    pub vcodec: String,
    pub acodec: String,
    pub size_bytes: u64,
}

// ---------- paths ----------
pub fn home_dir() -> VResult<PathBuf> {
    #[cfg(windows)]
    let key = "USERPROFILE";
    #[cfg(not(windows))]
    let key = "HOME";
    match env::var_os(key) {
        Some(p) if !p.is_empty() => Ok(PathBuf::from(p)),
        _ => Err("หา home directory ไม่เจอ (ตั้ง HOME / USERPROFILE)".into()),
    }
}

pub fn vdo_root() -> PathBuf {
    match env::var_os("VDO_ROOT") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home_dir().map(|h| h.join("VDO")).unwrap_or_else(|_| PathBuf::from("VDO")),
    }
}

/// โฟลเดอร์เก็บ yt-dlp/ffmpeg ที่โหลดมาเอง (override ด้วย VDO_BIN)
pub fn bin_dir() -> PathBuf {
    if let Some(p) = env::var_os("VDO_BIN") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let home = home_dir().unwrap_or_else(|_| PathBuf::from("."));
    #[cfg(windows)]
    {
        let base = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"));
        base.join("vdo-dl").join("bin")
    }
    #[cfg(target_os = "macos")]
    {
        home.join("Library").join("Application Support").join("vdo-dl").join("bin")
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"))
            .join("vdo-dl")
            .join("bin")
    }
}

/// หมวดที่มีอยู่ใน ~/VDO/ (ไว้ทำ dropdown ใน GUI) — เรียงตามตัวอักษร, ตัด tmp ออก
pub fn list_categories() -> Vec<String> {
    let mut cats = vec![];
    if let Ok(entries) = fs::read_dir(vdo_root()) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    if name != "tmp" && !name.starts_with('.') {
                        cats.push(name.to_string());
                    }
                }
            }
        }
    }
    cats.sort();
    cats
}

pub fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    }
}

/// ล้างอักขระที่ตั้งชื่อไฟล์ไม่ได้ (สำคัญบน Windows: \ / : * ? " < > |)
pub fn sanitize_title(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if (c as u32) < 0x20 => '-',
            c => c,
        })
        .collect();
    cleaned.trim().trim_end_matches('.').to_string()
}

// ---------- tool resolution ----------
fn runs(tool: &Path, arg: &str) -> bool {
    Command::new(tool)
        .arg(arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// หา tool: managed bin ก่อน → PATH → None
pub fn find_tool(name: &str, version_arg: &str) -> Option<PathBuf> {
    let managed = bin_dir().join(exe(name));
    if managed.is_file() {
        return Some(managed);
    }
    if runs(Path::new(name), version_arg) {
        return Some(PathBuf::from(name));
    }
    None
}

// ---------- download helpers (curl + tar) ----------
fn require_curl() -> VResult<()> {
    if runs(Path::new("curl"), "--version") {
        Ok(())
    } else {
        Err("ต้องมี curl เพื่อโหลดเครื่องมือครั้งแรก (Windows 10+/mac มีให้อยู่แล้ว)".into())
    }
}

/// โหลดไฟล์ด้วย curl (ลองทีละ URL จนสำเร็จ)
fn curl_download(urls: &[&str], dest: &Path, log: &Log) -> bool {
    for (i, url) in urls.iter().enumerate() {
        if i > 0 {
            log(&format!("ลอง mirror สำรอง: {}", url));
        }
        let status = Command::new("curl")
            .args(["-L", "--fail", "--progress-bar", "-o"])
            .arg(dest)
            .arg(url)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();
        if matches!(status, Ok(s) if s.success()) && dest.is_file() {
            return true;
        }
        let _ = fs::remove_file(dest);
    }
    false
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut perm = meta.permissions();
        perm.set_mode(0o755);
        let _ = fs::set_permissions(path, perm);
    }
}
#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

#[cfg(windows)]
fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().map(|f| f == name).unwrap_or(false) {
                return Some(path);
            }
        }
    }
    None
}

// ---------- provisioning ----------
fn ytdlp_asset() -> &'static str {
    if cfg!(windows) {
        "yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else {
        "yt-dlp_linux"
    }
}

fn provision_ytdlp(log: &Log) -> VResult<PathBuf> {
    require_curl()?;
    let dir = bin_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("สร้าง {} ไม่ได้: {}", dir.display(), e))?;
    let dest = dir.join(exe("yt-dlp"));
    let url = format!(
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/{}",
        ytdlp_asset()
    );
    log(&format!("โหลด yt-dlp ครั้งแรก → {}", dest.display()));
    if !curl_download(&[&url], &dest, log) {
        return Err("โหลด yt-dlp ไม่สำเร็จ".into());
    }
    make_executable(&dest);
    log("ได้ yt-dlp แล้ว");
    Ok(dest)
}

#[cfg(windows)]
fn provision_ffmpeg_bundle(log: &Log) -> VResult<()> {
    require_curl()?;
    if !runs(Path::new("tar"), "--version") {
        return Err("ต้องมี tar เพื่อแตกไฟล์ ffmpeg (Windows 10+ มีให้อยู่แล้ว)".into());
    }
    let dir = bin_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("สร้าง {} ไม่ได้: {}", dir.display(), e))?;

    let zip = dir.join("_ffmpeg.zip");
    let urls = [
        "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
        "https://github.com/BtbN/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl.zip",
    ];
    log("โหลด ffmpeg ครั้งแรก (อาจใช้เวลาสักครู่) ...");
    if !curl_download(&urls, &zip, log) {
        return Err("โหลด ffmpeg ไม่สำเร็จ".into());
    }

    let extract = dir.join("_ffmpeg_x");
    let _ = fs::remove_dir_all(&extract);
    fs::create_dir_all(&extract).map_err(|e| format!("สร้างโฟลเดอร์ extract ไม่ได้: {}", e))?;
    let status = Command::new("tar").arg("-xf").arg(&zip).arg("-C").arg(&extract).status();
    if !matches!(status, Ok(s) if s.success()) {
        return Err("แตกไฟล์ ffmpeg ไม่สำเร็จ".into());
    }

    for tool in ["ffmpeg.exe", "ffprobe.exe"] {
        match find_file(&extract, tool) {
            Some(src) => {
                let dest = dir.join(tool);
                fs::copy(&src, &dest).map_err(|e| format!("คัดลอก {} ไม่ได้: {}", tool, e))?;
            }
            None => return Err(format!("ไม่เจอ {} ในไฟล์ที่โหลดมา", tool)),
        }
    }
    let _ = fs::remove_file(&zip);
    let _ = fs::remove_dir_all(&extract);
    log("ได้ ffmpeg + ffprobe แล้ว");
    Ok(())
}

#[cfg(not(windows))]
fn provision_ffmpeg_bundle(log: &Log) -> VResult<()> {
    if cfg!(target_os = "macos") {
        log("ffmpeg ใช้ไม่ได้ — ลองซ่อมด้วย brew reinstall homebrew/core/ffmpeg ...");
        let _ = Command::new("brew")
            .args(["reinstall", "homebrew/core/ffmpeg"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if runs(Path::new("ffmpeg"), "-version") {
            log("ซ่อม ffmpeg เรียบร้อย");
            return Ok(());
        }
        return Err("ซ่อม ffmpeg ไม่สำเร็จ — ลองรันเอง: brew reinstall homebrew/core/ffmpeg".into());
    }
    Err("ไม่มี ffmpeg — ติดตั้งด้วย: apt install ffmpeg (หรือ dnf/pacman ตาม distro)".into())
}

/// yt-dlp + ffmpeg + ffprobe ให้พร้อมใช้ (โหลดมาเองถ้าขาด)
pub fn ensure_tools(log: &Log) -> VResult<Tools> {
    let yt_dlp = match find_tool("yt-dlp", "--version") {
        Some(p) => p,
        None => provision_ytdlp(log)?,
    };

    let mut ffmpeg = find_tool("ffmpeg", "-version");
    let mut ffprobe = find_tool("ffprobe", "-version");
    if ffmpeg.is_none() || ffprobe.is_none() {
        provision_ffmpeg_bundle(log)?;
        ffmpeg = find_tool("ffmpeg", "-version");
        ffprobe = find_tool("ffprobe", "-version");
    }

    Ok(Tools {
        yt_dlp,
        ffmpeg: ffmpeg.ok_or("ยังหา ffmpeg ไม่เจอหลังติดตั้ง")?,
        ffprobe: ffprobe.ok_or("ยังหา ffprobe ไม่เจอหลังติดตั้ง")?,
    })
}

/// path ของ yt-dlp (โหลดมาเองถ้าขาด) — สำหรับ -F
pub fn ytdlp_path(log: &Log) -> VResult<PathBuf> {
    match find_tool("yt-dlp", "--version") {
        Some(p) => Ok(p),
        None => provision_ytdlp(log),
    }
}

// ---------- download ----------
fn ffmpeg_location_arg(cmd: &mut Command, ffmpeg: &Path) {
    if let Some(dir) = ffmpeg.parent() {
        if !dir.as_os_str().is_empty() {
            cmd.arg("--ffmpeg-location").arg(dir);
        }
    }
}

/// kill process ตาม pid (ใช้ตอนยกเลิกโหลด)
fn kill_pid(pid: u32) {
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
    #[cfg(not(windows))]
    let _ = Command::new("kill").arg(pid.to_string()).output();
}

/// โหลดตาม opts → (วิดีโอ merge mp4 / เสียง mp3). progress มาทาง stderr ("PROG:<pct>"), path ทาง stdout.
/// `cancel` ตั้ง true เมื่อไรก็ได้เพื่อยกเลิก (watcher จะ kill yt-dlp).
pub fn download(
    tools: &Tools,
    url: &str,
    tmp: &Path,
    opts: &DownloadOpts,
    cancel: &AtomicBool,
    on: &Progress,
) -> VResult<PathBuf> {
    fs::create_dir_all(tmp).map_err(|e| format!("สร้าง tmp ไม่ได้: {}", e))?;
    // ชื่อไฟล์จดจำง่าย: ใช้ชื่อวิดีโอจริง (จำกัด 150 ไบต์) ถ้าไม่มีค่อย fallback เป็น id
    let out_template = tmp.join("%(title,id).150B.%(ext)s");

    let mut cmd = Command::new(&tools.yt_dlp);
    cmd.arg("--newline")
        .args(["--progress-template", "PROG:%(progress._percent_str)s"])
        .arg("-o")
        .arg(&out_template)
        .args(["--print", "after_move:filepath"]);

    if opts.audio {
        cmd.args(["-x", "--audio-format", "mp3"]);
    } else {
        let fmt = match opts.max_height {
            Some(h) => format!("bv*[height<={h}]+ba/b[height<={h}]"),
            None => "bv*+ba/b".to_string(),
        };
        cmd.args(["-f", &fmt, "--merge-output-format", "mp4"]);
    }
    ffmpeg_location_arg(&mut cmd, &tools.ffmpeg);

    let mut child = cmd
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("รัน yt-dlp ไม่ได้: {}", e))?;

    let pid = child.id();
    let stderr = child.stderr.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut last_path = String::new();
    let done = AtomicBool::new(false);

    std::thread::scope(|s| {
        // watcher: ยกเลิก → kill yt-dlp
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                if cancel.load(Ordering::Relaxed) {
                    kill_pid(pid);
                    break;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        });
        // stderr: progress + สถานะ
        s.spawn(|| {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let t = line.trim();
                if let Some(p) = t.strip_prefix("PROG:") {
                    let pct = p.trim().trim_end_matches('%').trim().parse::<f32>().unwrap_or(-1.0);
                    on(pct, t);
                } else if !t.is_empty() {
                    on(-1.0, t);
                }
            }
        });
        // stdout: path ของไฟล์ผลลัพธ์ (บรรทัด non-empty สุดท้าย)
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let t = line.trim();
            if !t.is_empty() {
                last_path = t.to_string();
            }
        }
        done.store(true, Ordering::Relaxed);
    });

    let status = child.wait().map_err(|e| format!("yt-dlp พัง: {}", e))?;
    if cancel.load(Ordering::Relaxed) {
        return Err("ยกเลิกการโหลด".into());
    }
    if !status.success() {
        return Err("yt-dlp โหลดไม่สำเร็จ".into());
    }
    if last_path.is_empty() {
        return Err("yt-dlp ไม่ได้พิมพ์ path ของไฟล์ผลลัพธ์".into());
    }
    let file = PathBuf::from(&last_path);
    if !file.is_file() {
        return Err(format!("หาไฟล์ผลลัพธ์ไม่เจอ ({})", file.display()));
    }
    Ok(file)
}

// ---------- verify ----------
fn probe(ffprobe: &Path, file: &Path, stream: &str, entry: &str) -> String {
    let out = Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", stream, "-show_entries"])
        .arg(format!("stream={}", entry))
        .args(["-of", "default=nw=1:nk=0"])
        .arg(file)
        .output();
    let prefix = format!("{}=", entry);
    if let Ok(out) = out {
        if let Ok(text) = String::from_utf8(out.stdout) {
            for line in text.lines() {
                if let Some(v) = line.trim().strip_prefix(&prefix) {
                    if !v.is_empty() {
                        return v.to_string();
                    }
                }
            }
        }
    }
    "?".to_string()
}

pub fn human_size(bytes: u64) -> String {
    const U: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{}{}", bytes, U[0])
    } else {
        format!("{:.1}{}", v, U[i])
    }
}

pub fn verify(ffprobe: &Path, file: &Path) -> VideoInfo {
    VideoInfo {
        width: probe(ffprobe, file, "v:0", "width"),
        height: probe(ffprobe, file, "v:0", "height"),
        vcodec: probe(ffprobe, file, "v:0", "codec_name"),
        acodec: probe(ffprobe, file, "a:0", "codec_name"),
        size_bytes: fs::metadata(file).map(|m| m.len()).unwrap_or(0),
    }
}

// ---------- file it ----------
/// ย้ายไฟล์เข้า ~/VDO/<category>/<title>.<ext จริงของต้นทาง> — คืน path ปลายทาง
pub fn file_into(src: &Path, category: &str, title: &str) -> VResult<PathBuf> {
    let cat = if category.trim().is_empty() { "ยังไม่จัดหมวด" } else { category.trim() };
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
    let dest = vdo_root().join(cat).join(format!("{}.{}", sanitize_title(title), ext));
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("สร้างโฟลเดอร์ปลายทางไม่ได้: {}", e))?;
    }
    if fs::rename(src, &dest).is_err() {
        fs::copy(src, &dest).map_err(|e| format!("ย้ายไฟล์ไม่สำเร็จ: {}", e))?;
        let _ = fs::remove_file(src);
    }
    Ok(dest)
}

// ---------- update ----------
pub fn update(log: &Log) -> VResult<()> {
    let managed_yt = bin_dir().join(exe("yt-dlp"));
    if managed_yt.is_file() {
        log("อัปเดต yt-dlp (yt-dlp -U) ...");
        let status = Command::new(&managed_yt).arg("-U").status();
        if !matches!(status, Ok(s) if s.success()) {
            log("self-update ไม่สำเร็จ — โหลดตัวใหม่แทน");
            let _ = fs::remove_file(&managed_yt);
            provision_ytdlp(log)?;
        }
    } else if find_tool("yt-dlp", "--version").is_none() {
        provision_ytdlp(log)?;
    } else {
        log("yt-dlp มาจาก PATH (เช่น brew) — อัปเดตผ่าน package manager ของระบบ");
    }

    if cfg!(windows) && bin_dir().join("ffmpeg.exe").is_file() {
        log("อัปเดต ffmpeg bundle ...");
        provision_ffmpeg_bundle(log)?;
    }
    Ok(())
}

// ---------- meta / OS helpers ----------
/// ดึง (ชื่อเรื่อง, URL รูป thumbnail) จาก URL โดยไม่โหลดวิดีโอ
pub fn probe_meta(url: &str) -> VResult<(String, String)> {
    let yt = find_tool("yt-dlp", "--version").ok_or("ยังไม่มี yt-dlp")?;
    let out = Command::new(yt)
        .args(["--skip-download", "--no-warnings", "--no-playlist"])
        .args(["--print", "%(title)s", "--print", "%(thumbnail)s"])
        .arg(url)
        .output()
        .map_err(|e| format!("รัน yt-dlp ไม่ได้: {}", e))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let title = lines.next().unwrap_or("").trim().to_string();
    let thumb = lines.next().unwrap_or("").trim().to_string();
    if title.is_empty() {
        return Err("ดึงข้อมูลวิดีโอไม่ได้ (URL ใช้ไม่ได้?)".into());
    }
    Ok((title, thumb))
}

/// ลบไฟล์ในดิสก์ (เฉพาะที่อยู่ใต้ ~/VDO เพื่อกันลบผิด)
pub fn delete_file(path: &str) -> VResult<()> {
    let p = Path::new(path);
    if !p.exists() {
        return Ok(()); // หายไปแล้วก็ถือว่าสำเร็จ
    }
    let root = vdo_root();
    let safe = p
        .canonicalize()
        .ok()
        .zip(root.canonicalize().ok())
        .map(|(f, r)| f.starts_with(&r))
        .unwrap_or(false);
    if !safe {
        return Err("ลบได้เฉพาะไฟล์ใน ~/VDO เท่านั้น".into());
    }
    fs::remove_file(p).map_err(|e| format!("ลบไฟล์ไม่ได้: {}", e))
}

/// เปิด file manager แล้วเลือกไฟล์นั้น (mac: Finder, win: Explorer, linux: เปิดโฟลเดอร์)
pub fn reveal_path(path: &str) -> VResult<()> {
    let p = Path::new(path);
    if !p.exists() {
        return Err("ไม่พบไฟล์".into());
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg("-R").arg(path).status();
    }
    #[cfg(windows)]
    {
        // explorer คืน exit code ≠ 0 แม้สำเร็จ → ไม่เช็ค status
        let _ = Command::new("explorer").arg(format!("/select,{}", path)).status();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = p.parent().unwrap_or(p);
        let _ = Command::new("xdg-open").arg(dir).status();
    }
    Ok(())
}

/// อ่านข้อความจาก clipboard ผ่าน OS tool — ไม่มี/ล้มเหลว = คืนค่าว่าง
pub fn read_clipboard() -> String {
    let try_cmd = |prog: &str, args: &[&str]| -> Option<String> {
        let out = Command::new(prog).args(args).output().ok()?;
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            None
        }
    };
    #[cfg(target_os = "macos")]
    {
        try_cmd("pbpaste", &[]).unwrap_or_default()
    }
    #[cfg(windows)]
    {
        try_cmd("powershell", &["-NoProfile", "-Command", "Get-Clipboard"]).unwrap_or_default()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        try_cmd("wl-paste", &[])
            .or_else(|| try_cmd("xclip", &["-selection", "clipboard", "-o"]))
            .unwrap_or_default()
    }
}
