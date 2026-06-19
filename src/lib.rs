// vdo-dl core — logic ที่ใช้ร่วมกันระหว่าง CLI (src/main.rs) และ Tauri app (src-tauri/)
//
// ทุกฟังก์ชันคืน Result<_, String> (ไม่มี die/exit) เพื่อให้ GUI จัดการ error ได้โดยไม่ crash.
// สถานะ/ความคืบหน้าส่งออกผ่าน callback — CLI พิมพ์ลง stderr, Tauri แปลงเป็น event.
//
// zero-dependency: ห่อ yt-dlp + ffmpeg, โหลดผ่าน curl, แตก zip ด้วย tar ที่มากับ OS.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub type VResult<T> = Result<T, String>;

/// container ของไฟล์วิดีโอที่ merge ออกมา
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub enum Container {
    #[default]
    Mp4,
    Mkv,
}
impl Container {
    pub fn as_str(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "mkv",
        }
    }
    /// parse จาก string ที่ frontend ส่งมา (ไม่รู้จัก = default mp4)
    pub fn parse(s: &str) -> Container {
        match s.trim().to_lowercase().as_str() {
            "mkv" => Container::Mkv,
            _ => Container::Mp4,
        }
    }
}

/// รูปแบบไฟล์เสียงตอน extract
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub enum AudioFmt {
    #[default]
    Mp3,
    M4a,
    Ogg,
}
impl AudioFmt {
    pub fn as_str(self) -> &'static str {
        match self {
            AudioFmt::Mp3 => "mp3",
            AudioFmt::M4a => "m4a",
            AudioFmt::Ogg => "ogg",
        }
    }
    /// parse จาก string ที่ frontend ส่งมา (ไม่รู้จัก = default mp3)
    pub fn parse(s: &str) -> AudioFmt {
        match s.trim().to_lowercase().as_str() {
            "m4a" => AudioFmt::M4a,
            "ogg" => AudioFmt::Ogg,
            _ => AudioFmt::Mp3,
        }
    }
}

/// แหล่งคุกกี้สำหรับเนื้อหาที่ต้องล็อกอิน (เช่น คอร์สที่ enroll). หมายเหตุความเป็นส่วนตัว:
/// เก็บแค่ "ที่มา" (ชื่อเบราว์เซอร์/path ไฟล์) — yt-dlp อ่านตัวคุกกี้สดเอง เราไม่แตะ/ไม่เก็บ token
#[derive(Clone, Default)]
pub enum Cookies {
    #[default]
    None,
    Browser(String), // brave|chrome|chromium|edge|firefox|opera|safari|vivaldi|whale
    File(PathBuf),   // cookies.txt (Netscape format)
}
impl Cookies {
    /// สร้างจากค่าที่ frontend ส่งมา (เลือก browser ก่อน, ถ้าไม่มีลอง file)
    pub fn from(browser: Option<String>, file: Option<String>) -> Cookies {
        match (browser, file) {
            (Some(b), _) if !b.trim().is_empty() => Cookies::Browser(b.trim().to_string()),
            (_, Some(f)) if !f.trim().is_empty() => Cookies::File(PathBuf::from(f)),
            _ => Cookies::None,
        }
    }
}

/// args ของ yt-dlp สำหรับคุกกี้ (ว่าง = ไม่ใช้)
pub fn cookie_args(c: &Cookies) -> Vec<String> {
    match c {
        Cookies::None => vec![],
        Cookies::Browser(b) => vec!["--cookies-from-browser".into(), b.clone()],
        Cookies::File(p) => vec!["--cookies".into(), p.display().to_string()],
    }
}

/// ตัวเลือกการโหลด. หมายเหตุ: ไม่ derive Copy แล้ว เพราะมี sub_langs:String
#[derive(Clone, Default)]
pub struct DownloadOpts {
    pub audio: bool,
    pub max_height: Option<u32>,    // จำกัดความสูง (None = สูงสุด)
    pub container: Container,       // วิดีโอ: --merge-output-format
    pub audio_fmt: AudioFmt,        // เสียง: --audio-format
    pub audio_quality: Option<u8>,  // --audio-quality 0(best)..10 (None = ค่า default ของ yt-dlp)
    pub subs: bool,                 // ดาวน์โหลด+ฝังคำบรรยาย (เฉพาะวิดีโอ)
    pub sub_langs: String,          // ภาษาซับ เช่น "en,th" หรือ "all" (ว่าง = ข้าม แม้ subs=true)
    pub playlist: bool,             // true = โหลดทั้ง playlist (default false = คลิปเดียว)
    pub cookies: Cookies,           // คุกกี้สำหรับเนื้อหาที่ต้องล็อกอิน
    pub wait_for_video: Option<u32>, // รอไลฟ์/พรีเมียร์ที่ตั้งเวลา: ช่วงเวลา poll (วินาที); None = ไม่รอ
}

/// args ของ yt-dlp สำหรับรอไลฟ์/พรีเมียร์ที่ตั้งเวลาไว้ (None = ไม่รอ).
/// ค่า = ช่วง retry เป็นวินาที (ส่งเป็น MIN ของ --wait-for-video MIN[-MAX]).
pub fn wait_for_video_args(secs: Option<u32>) -> Vec<String> {
    match secs {
        Some(s) => vec!["--wait-for-video".into(), s.to_string()],
        None => vec![],
    }
}

/// สร้าง args ของ yt-dlp สำหรับ format + subtitle (แยกเป็นฟังก์ชันบริสุทธิ์เพื่อ unit-test)
pub fn build_format_args(opts: &DownloadOpts) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    if opts.audio {
        a.push("-x".into());
        a.push("--audio-format".into());
        a.push(opts.audio_fmt.as_str().into());
        if let Some(q) = opts.audio_quality {
            a.push("--audio-quality".into());
            a.push(q.to_string());
        }
        return a; // เสียงไม่มี container/subtitle
    }

    let fmt = match opts.max_height {
        Some(h) => format!("bv*[height<={h}]+ba/b[height<={h}]"),
        None => "bv*+ba/b".to_string(),
    };
    a.push("-f".into());
    a.push(fmt);
    a.push("--merge-output-format".into());
    a.push(opts.container.as_str().into());

    if opts.subs && !opts.sub_langs.trim().is_empty() {
        a.push("--write-subs".into());
        a.push("--write-auto-subs".into());
        a.push("--sub-langs".into());
        a.push(opts.sub_langs.trim().into());
        a.push("--embed-subs".into());
        a.push("--convert-subs".into());
        a.push("srt".into());
    }
    a
}

/// callback รับข้อความสถานะ (เช่น "โหลด yt-dlp ครั้งแรก…")
pub type Log<'a> = dyn Fn(&str) + Sync + 'a;

/// เหตุการณ์ระหว่างโหลด — index = ลำดับใน playlist (None = วิดีโอเดี่ยว/ไม่ใช่ playlist).
/// รองรับ Option B: GUI map event กลับไปแถวลูกตาม index ได้.
pub enum DlEvent<'a> {
    /// ความคืบหน้าของ item ปัจจุบัน (percent 0..100)
    Progress { index: Option<u32>, pct: f32 },
    /// ข้อความสถานะ/คำเตือนดิบจาก yt-dlp
    Status { index: Option<u32>, line: &'a str },
    /// item หนึ่งโหลด+ย้ายเสร็จแล้ว (path ใน tmp) + provenance ต้นทาง
    ItemDone { index: Option<u32>, path: PathBuf, meta: ItemMeta },
}

/// metadata ต้นทาง (provenance) ที่ yt-dlp พ่นมาตอน item เสร็จ — เก็บลง index DB เพื่อค้นย้อนหลัง.
/// field ว่าง = yt-dlp ไม่มีค่านั้น (เช่นบางเว็บไม่มี uploader/upload_date)
#[derive(Clone, Debug, Default)]
pub struct ItemMeta {
    pub video_id: String,
    pub title: String,
    pub webpage_url: String,
    pub uploader: String,
    pub upload_date: String,
    pub duration: String,
    pub extractor: String,
}

/// item ที่โหลด+ย้ายลง tmp สำเร็จ — path ใน tmp + provenance (ใช้ต่อในชั้น caller: file_into แล้ว index)
#[derive(Clone, Debug)]
pub struct DownloadedItem {
    pub index: Option<u32>,
    pub path: PathBuf,
    pub meta: ItemMeta,
}
/// callback รับ DlEvent ระหว่างโหลด (เรียกจากหลาย thread ได้ → ต้อง Sync)
pub type OnEvent<'a> = dyn Fn(DlEvent) + Sync + 'a;

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

/// ไดเรกทอรีมาตรฐานที่ tool มักอยู่ — เผื่อ GUI app (เปิดจาก Finder) ที่ได้ PATH แคบ
/// (`/usr/bin:/bin:/usr/sbin:/sbin`) ไม่มี Homebrew. ต้องหาเจอด้วย absolute path
fn extra_tool_dirs() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &["/usr/local/bin", "/usr/bin", "/snap/bin"]
    }
    #[cfg(windows)]
    {
        &[]
    }
}

/// หา tool: managed bin → PATH → ไดเรกทอรีมาตรฐาน (Homebrew ฯลฯ) → None
pub fn find_tool(name: &str, version_arg: &str) -> Option<PathBuf> {
    let managed = bin_dir().join(exe(name));
    if managed.is_file() {
        return Some(managed);
    }
    if runs(Path::new(name), version_arg) {
        return Some(PathBuf::from(name));
    }
    // GUI app บน macOS ได้ PATH แคบจาก launchd → /opt/homebrew/bin หาไม่เจอผ่าน PATH
    for dir in extra_tool_dirs() {
        let p = Path::new(dir).join(exe(name));
        if p.is_file() && runs(&p, version_arg) {
            return Some(p);
        }
    }
    None
}

/// เพิ่ม extra_tool_dirs เข้าหัว PATH ของ child process. สำคัญสำหรับ yt-dlp: มันต้องหา
/// JS runtime (deno) ใน PATH ของตัวเองเพื่อแก้ JS challenge ของ YouTube. GUI app ที่เปิด
/// จาก Finder ได้ PATH แคบ (ไม่มี /opt/homebrew/bin) → หา deno ไม่เจอ → ได้ format ไม่ครบ
/// → "Requested format is not available". เพิ่ม PATH ให้ครอบ Homebrew ฯลฯ จึงแก้ได้.
pub fn prepend_tool_path(cmd: &mut Command) {
    let cur = env::var_os("PATH").unwrap_or_default();
    let extra = extra_tool_dirs().iter().map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = extra.collect();
    dirs.extend(env::split_paths(&cur));
    if let Ok(joined) = env::join_paths(dirs) {
        cmd.env("PATH", joined);
    }
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

/// แปลงค่า playlist_index จาก yt-dlp ("NA"/ว่าง = ไม่ใช่ playlist) → Option<u32>
fn parse_idx(s: &str) -> Option<u32> {
    match s.trim() {
        "" | "NA" => None,
        n => n.parse().ok(),
    }
}

/// แปลง field จาก yt-dlp --print เป็น String — "NA"/ว่าง = ไม่มีค่า → ""
fn na(s: Option<&str>) -> String {
    match s.map(str::trim) {
        Some(v) if !v.is_empty() && v != "NA" => v.to_string(),
        _ => String::new(),
    }
}

/// โหลดตาม opts. คืน Vec ของ (playlist_index, path ใน tmp) ตามลำดับที่โหลดเสร็จ
/// (วิดีโอเดี่ยว = 1 รายการ index=None; playlist = หลายรายการ). ส่ง DlEvent ระหว่างทาง.
///
/// Protocol (ผ่าน stdout, verify แล้ว — ดู Phase 2.0):
///   PROG:::<percent>:::<playlist_index>   ความคืบหน้าต่อ item
///   DONE:::<playlist_index>:::<filepath>  item โหลด+ย้ายเสร็จ
/// `cancel` ตั้ง true เมื่อไรก็ได้เพื่อยกเลิก (watcher จะ kill yt-dlp).
pub fn download(
    tools: &Tools,
    url: &str,
    tmp: &Path,
    opts: &DownloadOpts,
    cancel: &AtomicBool,
    on: &OnEvent,
) -> VResult<Vec<DownloadedItem>> {
    fs::create_dir_all(tmp).map_err(|e| format!("สร้าง tmp ไม่ได้: {}", e))?;
    // playlist: ใส่เลขลำดับนำหน้าชื่อไฟล์; เดี่ยว: ใช้ชื่อวิดีโอจริง (fallback id)
    let out_template = if opts.playlist {
        tmp.join("%(playlist_index)02d - %(title,id).120B.%(ext)s")
    } else {
        tmp.join("%(title,id).150B.%(ext)s")
    };

    let mut cmd = Command::new(&tools.yt_dlp);
    prepend_tool_path(&mut cmd); // ให้ yt-dlp หา deno (JS runtime) เจอแม้ PATH แคบจาก GUI
    // --progress: บังคับให้พ่น progress แม้ถูก pipe (ไม่ใช่ TTY) ไม่งั้น yt-dlp เงียบ
    // หมายเหตุ: progress-template + --print ออกที่ "stdout" (ไม่ใช่ stderr)
    cmd.arg("--newline")
        .arg("--progress")
        .args([
            "--progress-template",
            "PROG:::%(progress._percent_str)s:::%(info.playlist_index)s",
        ])
        .arg("-o")
        .arg(&out_template)
        // DONE line พก provenance ติดมาด้วย (ดึงฟรีตอน item เสร็จ ไม่ต้องเรียก yt-dlp ซ้ำ);
        // filepath อยู่ท้ายสุดกัน ::: ในชื่อ path ทำ parser เพี้ยน (path ไม่มี ::: อยู่แล้ว)
        .args([
            "--print",
            "after_move:DONE:::%(playlist_index)s:::%(id)s:::%(title)s:::%(webpage_url)s:::%(uploader)s:::%(upload_date)s:::%(duration)s:::%(extractor)s:::%(filepath)s",
        ]);

    if opts.playlist {
        // -i: item ที่พัง (เช่น DRM ใน Phase 3) ให้ข้าม ไม่ล้มทั้ง playlist
        cmd.arg("--yes-playlist").arg("-i");
    } else {
        cmd.arg("--no-playlist");
    }

    cmd.args(build_format_args(opts));
    cmd.args(cookie_args(&opts.cookies));
    cmd.args(wait_for_video_args(opts.wait_for_video));
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
    let mut items: Vec<DownloadedItem> = Vec::new();
    let done = AtomicBool::new(false);
    // เก็บบรรทัด ERROR ล่าสุดไว้แปลงเป็นข้อความที่อ่านง่ายตอนล้มเหลว
    let last_err = std::sync::Mutex::new(String::new());

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
        // stderr: ข้อความสถานะ/คำเตือน (progress ไม่ได้มาทางนี้)
        s.spawn(|| {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let t = line.trim();
                if !t.is_empty() {
                    let low = t.to_lowercase();
                    if low.contains("error") || low.contains("drm") || low.contains("forbidden") {
                        *last_err.lock().unwrap() = t.to_string();
                    }
                    on(DlEvent::Status { index: None, line: t });
                }
            }
        });
        // stdout: PROG:::pct:::idx (progress) + DONE:::idx:::path (item เสร็จ)
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("PROG:::") {
                let mut f = rest.splitn(2, ":::");
                let pct = f
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_end_matches('%')
                    .trim()
                    .parse::<f32>()
                    .unwrap_or(-1.0);
                let index = parse_idx(f.next().unwrap_or(""));
                on(DlEvent::Progress { index, pct });
            } else if let Some(rest) = t.strip_prefix("DONE:::") {
                let mut f = rest.splitn(9, ":::");
                let index = parse_idx(f.next().unwrap_or(""));
                let meta = ItemMeta {
                    video_id: na(f.next()),
                    title: na(f.next()),
                    webpage_url: na(f.next()),
                    uploader: na(f.next()),
                    upload_date: na(f.next()),
                    duration: na(f.next()),
                    extractor: na(f.next()),
                };
                let path = PathBuf::from(f.next().unwrap_or("").trim());
                if path.is_file() {
                    on(DlEvent::ItemDone {
                        index,
                        path: path.clone(),
                        meta: meta.clone(),
                    });
                    items.push(DownloadedItem { index, path, meta });
                }
            }
        }
        done.store(true, Ordering::Relaxed);
    });

    let status = child.wait().map_err(|e| format!("yt-dlp พัง: {}", e))?;
    if cancel.load(Ordering::Relaxed) {
        return Err("ยกเลิกการโหลด".into());
    }
    // playlist ใช้ -i: บาง item พังได้แต่ถ้าได้อย่างน้อย 1 ไฟล์ถือว่าสำเร็จบางส่วน
    if items.is_empty() {
        let raw = last_err.lock().unwrap().clone();
        if !status.success() || !raw.is_empty() {
            return Err(friendly_error(&raw));
        }
        return Err("yt-dlp ไม่ได้พิมพ์ path ของไฟล์ผลลัพธ์".into());
    }
    Ok(items)
}

/// แปลง error ดิบจาก yt-dlp เป็นข้อความที่ผู้ใช้เข้าใจ (DRM / 403 / Udemy / ต้องล็อกอิน)
pub fn friendly_error(raw: &str) -> String {
    let low = raw.to_lowercase();
    if low.contains("drm") {
        "วิดีโอมี DRM — ดาวน์โหลดไม่ได้ (ดูในแอปทางการเท่านั้น)".into()
    } else if low.contains("udemy")
        && (low.contains("403") || low.contains("forbidden") || low.contains("course id"))
    {
        "Udemy Business โหลดไม่ได้ — Udemy บล็อก yt-dlp (403) และเนื้อหามักมี DRM".into()
    } else if low.contains("403") || low.contains("forbidden") {
        "เซิร์ฟเวอร์บล็อก (403 Forbidden) — เนื้อหานี้โหลดไม่ได้".into()
    } else if low.contains("login")
        || low.contains("sign in")
        || low.contains("private")
        || low.contains("members-only")
        || low.contains("registered users")
    {
        "ต้องล็อกอิน — ลองตั้งคุกกี้ในเมนูตั้งค่า (บัญชี/คุกกี้)".into()
    } else if low.contains("live event will begin")
        || low.contains("premiere will begin")
        || low.contains("premieres in")
        || low.contains("will begin in")
        || low.contains("this live event")
    {
        "ไลฟ์/พรีเมียร์ยังไม่เริ่ม — รอจนถ่ายทอดสดเริ่มแล้วค่อยโหลดอีกครั้ง".into()
    } else if low.contains("live stream recording is not available")
        || low.contains("this live stream recording")
    {
        "ไลฟ์จบแล้วแต่ยังไม่มีไฟล์ย้อนหลัง — รอ YouTube ประมวลผลวิดีโอก่อน แล้วค่อยลองใหม่".into()
    } else if low.contains("requested format is not available") {
        "ไม่พบรูปแบบที่ขอ — YouTube ต้องใช้ JS runtime (deno) สกัดวิดีโอ; ติดตั้งด้วย `brew install deno` แล้วลองใหม่".into()
    } else if !raw.trim().is_empty() {
        format!("yt-dlp: {}", raw.trim_start_matches("ERROR:").trim())
    } else {
        "yt-dlp โหลดไม่สำเร็จ".into()
    }
}

// ---------- index (provenance DB ผ่าน sqlite3 CLI — คง zero Rust dep ของ core) ----------
//
// เก็บ source URL + metadata ทุกครั้งที่โหลด ลง DB กลางที่ <vdo_root>/.vdo-dl/index.db
// เพื่อค้นย้อนหลังได้ (ดู subcommand `search`/`backfill` ใน CLI). shell out ไป sqlite3
// ที่มากับ OS เหมือนวิธีเรียก yt-dlp/ffmpeg — ไม่ดึง Rust crate เข้ามา.
// best-effort: เครื่องไม่มี sqlite3 → ข้ามเงียบ ไม่ให้การโหลดพัง.

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS downloads (\
    id INTEGER PRIMARY KEY AUTOINCREMENT, \
    source_url TEXT NOT NULL, video_id TEXT, webpage_url TEXT, \
    title TEXT, category TEXT, dest_path TEXT UNIQUE, ext TEXT, \
    width TEXT, height TEXT, vcodec TEXT, acodec TEXT, size_bytes INTEGER, \
    uploader TEXT, upload_date TEXT, duration TEXT, extractor TEXT, \
    playlist_index INTEGER, downloaded_at INTEGER);";

/// path ของ DB กลาง: <vdo_root>/.vdo-dl/index.db
pub fn index_db_path() -> PathBuf {
    vdo_root().join(".vdo-dl").join("index.db")
}

/// หา sqlite3 (managed bin → PATH). None = เครื่องไม่มี → caller ข้าม indexing แบบเงียบ
pub fn sqlite3_path() -> Option<PathBuf> {
    find_tool("sqlite3", "-version")
}

/// escape เป็น SQL string literal ของ sqlite (double single-quote) — กัน injection จากชื่อไฟล์/ชื่อเรื่อง
fn sql_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn now_epoch() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// รัน SQL ผ่าน stdin ของ sqlite3 — คืน stdout. สร้างโฟลเดอร์ DB ให้ถ้ายังไม่มี
fn run_sql(db: &Path, sql: &str) -> VResult<String> {
    let sqlite = sqlite3_path().ok_or("ไม่มี sqlite3 บนเครื่องนี้")?;
    if let Some(parent) = db.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("สร้างโฟลเดอร์ index ไม่ได้: {}", e))?;
    }
    let mut child = Command::new(sqlite)
        .arg(db)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("รัน sqlite3 ไม่ได้: {}", e))?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(sql.as_bytes())
        .map_err(|e| format!("เขียน SQL ไม่ได้: {}", e))?;
    let out = child.wait_with_output().map_err(|e| format!("sqlite3 พัง: {}", e))?;
    if !out.status.success() {
        return Err(format!("sqlite3: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// แกนกลางของ index_record — ระบุ downloaded_at เองได้ (backfill ใช้ mtime ของไฟล์)
#[allow(clippy::too_many_arguments)]
fn index_record_at(
    source_url: &str,
    index: Option<u32>,
    meta: &ItemMeta,
    title: &str,
    category: &str,
    dest: &Path,
    info: &VideoInfo,
    at: i64,
) -> VResult<()> {
    let ext = dest.extension().and_then(|e| e.to_str()).unwrap_or("");
    let plidx = index.map(|i| i.to_string()).unwrap_or_else(|| "NULL".to_string());
    // SCHEMA นำหน้าทุกครั้ง (idempotent, ถูก) — ไม่ต้อง init แยก
    let sql = format!(
        "{schema}\nINSERT INTO downloads \
         (source_url, video_id, webpage_url, title, category, dest_path, ext, \
          width, height, vcodec, acodec, size_bytes, uploader, upload_date, duration, extractor, \
          playlist_index, downloaded_at) \
         VALUES ({su},{vid},{wu},{ti},{cat},{dp},{ex},{w},{h},{vc},{ac},{sz},{up},{ud},{dur},{exr},{pi},{at}) \
         ON CONFLICT(dest_path) DO UPDATE SET \
          source_url=excluded.source_url, video_id=excluded.video_id, webpage_url=excluded.webpage_url, \
          title=excluded.title, category=excluded.category, ext=excluded.ext, \
          width=excluded.width, height=excluded.height, vcodec=excluded.vcodec, acodec=excluded.acodec, \
          size_bytes=excluded.size_bytes, uploader=excluded.uploader, upload_date=excluded.upload_date, \
          duration=excluded.duration, extractor=excluded.extractor, playlist_index=excluded.playlist_index, \
          downloaded_at=excluded.downloaded_at;",
        schema = SCHEMA,
        su = sql_quote(source_url),
        vid = sql_quote(&meta.video_id),
        wu = sql_quote(&meta.webpage_url),
        ti = sql_quote(title),
        cat = sql_quote(category),
        dp = sql_quote(&dest.display().to_string()),
        ex = sql_quote(ext),
        w = sql_quote(&info.width),
        h = sql_quote(&info.height),
        vc = sql_quote(&info.vcodec),
        ac = sql_quote(&info.acodec),
        sz = info.size_bytes,
        up = sql_quote(&meta.uploader),
        ud = sql_quote(&meta.upload_date),
        dur = sql_quote(&meta.duration),
        exr = sql_quote(&meta.extractor),
        pi = plidx,
        at = at,
    );
    run_sql(&index_db_path(), &sql).map(|_| ())
}

/// บันทึก 1 รายการลง index (downloaded_at = ตอนนี้). best-effort: ไม่มี sqlite3 → Ok เงียบ.
/// source_url = URL ที่ผู้ใช้ใส่ (playlist = URL ทั้งชุด); meta.webpage_url = URL ต่อ item จริง
pub fn index_record(
    source_url: &str,
    index: Option<u32>,
    meta: &ItemMeta,
    title: &str,
    category: &str,
    dest: &Path,
    info: &VideoInfo,
) -> VResult<()> {
    if sqlite3_path().is_none() {
        return Ok(());
    }
    index_record_at(source_url, index, meta, title, category, dest, info, now_epoch())
}

fn is_video_file(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(),
        Some("mp4" | "mkv" | "webm" | "mov" | "m4v" | "avi" | "flv" | "mp3" | "m4a" | "opus" | "wav")
    )
}

/// เดินโฟลเดอร์ปลายทางหาไฟล์วิดีโอ (ข้าม .vdo-dl และ tmp)
fn collect_videos(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if name == ".vdo-dl" || name == "tmp" {
                continue;
            }
            collect_videos(&p, out);
        } else if is_video_file(&p) {
            out.push(p);
        }
    }
}

/// สแกนไฟล์วิดีโอใน vdo_root ที่ยังไม่อยู่ใน index แล้วเพิ่มเข้า. provenance ของเก่ากู้ได้แค่จากตัวไฟล์
/// (ความละเอียด/ขนาด/หมวด=โฟลเดอร์ชั้นแรก/ชื่อ/เวลา=mtime) — source URL กู้ไม่ได้ จึงเว้นว่าง.
/// คืนจำนวนรายการที่เพิ่มใหม่
pub fn backfill_index(ffprobe: &Path, on: &Log) -> VResult<usize> {
    if sqlite3_path().is_none() {
        return Err("ไม่มี sqlite3 บนเครื่องนี้ — ติดตั้งก่อน (mac: มากับ OS)".into());
    }
    let db = index_db_path();
    run_sql(&db, SCHEMA)?;
    let existing_raw = run_sql(&db, ".mode list\nSELECT dest_path FROM downloads;")?;
    let existing: std::collections::HashSet<String> =
        existing_raw.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();

    let root = vdo_root();
    let mut files = Vec::new();
    collect_videos(&root, &mut files);

    let mut added = 0usize;
    for f in files {
        let p = f.display().to_string();
        if existing.contains(&p) {
            continue;
        }
        let category = f
            .strip_prefix(&root)
            .ok()
            .and_then(|r| r.components().next())
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let info = verify(ffprobe, &f);
        let mtime = fs::metadata(&f)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or_else(now_epoch);
        if index_record_at("", None, &ItemMeta::default(), &title, &category, &f, &info, mtime).is_ok() {
            added += 1;
            on(&format!("เพิ่ม: {}", p));
        }
    }
    Ok(added)
}

/// ค้น index — match title/uploader/category/url/extractor. q ว่าง = แสดงล่าสุดทั้งหมด. คืนตารางพร้อมพิมพ์
pub fn search_index(q: &str) -> VResult<String> {
    let where_clause = if q.trim().is_empty() {
        String::new()
    } else {
        let p = sql_quote(&format!("%{}%", q.trim()));
        format!(
            "WHERE title LIKE {p} OR uploader LIKE {p} OR category LIKE {p} \
             OR source_url LIKE {p} OR webpage_url LIKE {p} OR extractor LIKE {p}"
        )
    };
    let sql = format!(
        "{schema}\n.headers on\n.mode column\nSELECT \
         datetime(downloaded_at,'unixepoch','localtime') AS time, \
         category AS cat, substr(title,1,40) AS title, uploader, extractor AS src, \
         source_url AS url \
         FROM downloads {where_clause} ORDER BY downloaded_at DESC LIMIT 50;",
        schema = SCHEMA
    );
    run_sql(&index_db_path(), &sql)
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
    move_file(src, &dest)?;
    Ok(dest)
}

/// ย้ายไฟล์เข้า ~/VDO/<category>/<subfolder>/ โดย "คงชื่อไฟล์เดิม" (มีเลขลำดับนำหน้า)
/// ใช้กับ playlist — แต่ละ item ชื่อต่างกันอยู่แล้ว ไม่ใช้ title เดียวร่วมกัน. คืน path ปลายทาง
pub fn file_into_dir(src: &Path, category: &str, subfolder: &str) -> VResult<PathBuf> {
    let cat = if category.trim().is_empty() { "ยังไม่จัดหมวด" } else { category.trim() };
    let name = src.file_name().ok_or("ไฟล์ต้นทางไม่มีชื่อ")?;
    let dest = vdo_root().join(cat).join(sanitize_title(subfolder)).join(name);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("สร้างโฟลเดอร์ปลายทางไม่ได้: {}", e))?;
    }
    move_file(src, &dest)?;
    Ok(dest)
}

/// ย้ายไฟล์ (rename ก่อน, ข้าม filesystem ค่อย copy+remove)
fn move_file(src: &Path, dest: &Path) -> VResult<()> {
    if fs::rename(src, dest).is_err() {
        fs::copy(src, dest).map_err(|e| format!("ย้ายไฟล์ไม่สำเร็จ: {}", e))?;
        let _ = fs::remove_file(src);
    }
    Ok(())
}

/// เขียนไฟล์ playlist .m3u (รายชื่อไฟล์ relative) ในโฟลเดอร์ปลายทาง — คืน path ของ .m3u
pub fn write_m3u(dir: &Path, name: &str, files: &[PathBuf]) -> VResult<PathBuf> {
    let m3u = dir.join(format!("{}.m3u", sanitize_title(name)));
    let mut content = String::from("#EXTM3U\n");
    for f in files {
        if let Some(n) = f.file_name().and_then(|x| x.to_str()) {
            content.push_str(n);
            content.push('\n');
        }
    }
    fs::write(&m3u, content).map_err(|e| format!("เขียน .m3u ไม่ได้: {}", e))?;
    Ok(m3u)
}

/// ดึงข้อมูล playlist แบบเร็ว (ไม่โหลดวิดีโอ) → (ชื่อ playlist, รายชื่อ title แต่ละ entry)
/// ใช้ทำ subfolder + pre-create แถวลูกใน GUI (Option B). ต้องส่ง cookies ด้วยถ้า playlist
/// อยู่หลัง login (เช่น udemy:course)
pub fn probe_playlist(url: &str, cookies: &Cookies) -> VResult<(String, Vec<String>)> {
    let yt = find_tool("yt-dlp", "--version").ok_or("ยังไม่มี yt-dlp")?;
    let mut cmd = Command::new(yt);
    prepend_tool_path(&mut cmd);
    cmd.args(["--flat-playlist", "--no-warnings"])
        .args(["--print", "%(playlist_title)s:::%(title)s"])
        .args(cookie_args(cookies))
        .arg(url);
    let out = cmd.output().map_err(|e| format!("รัน yt-dlp ไม่ได้: {}", e))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut pl_title = String::new();
    let mut entries = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let mut f = line.splitn(2, ":::");
        let t = f.next().unwrap_or("").trim();
        if pl_title.is_empty() && !t.is_empty() && t != "NA" {
            pl_title = t.to_string();
        }
        entries.push(f.next().unwrap_or("").trim().to_string());
    }
    if entries.is_empty() {
        return Err("ดึงข้อมูล playlist ไม่ได้ (URL ใช้ไม่ได้ หรือไม่ใช่ playlist?)".into());
    }
    if pl_title.is_empty() {
        pl_title = "playlist".to_string();
    }
    Ok((pl_title, entries))
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
pub fn probe_meta(url: &str, cookies: &Cookies) -> VResult<(String, String)> {
    let yt = find_tool("yt-dlp", "--version").ok_or("ยังไม่มี yt-dlp")?;
    let mut cmd = Command::new(yt);
    prepend_tool_path(&mut cmd);
    cmd.args(["--skip-download", "--no-warnings", "--no-playlist"])
        .args(["--print", "%(title)s", "--print", "%(thumbnail)s"])
        .args(cookie_args(cookies))
        .arg(url);
    let out = cmd.output().map_err(|e| format!("รัน yt-dlp ไม่ได้: {}", e))?;
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

// ---------- tests ----------
#[cfg(test)]
mod tests {
    use super::*;

    fn has_pair(args: &[String], key: &str, val: &str) -> bool {
        args.windows(2).any(|w| w[0] == key && w[1] == val)
    }

    #[test]
    fn video_default_is_best_mp4() {
        let a = build_format_args(&DownloadOpts::default());
        assert!(has_pair(&a, "-f", "bv*+ba/b"));
        assert!(has_pair(&a, "--merge-output-format", "mp4"));
        assert!(!a.iter().any(|s| s == "-x"));
    }

    #[test]
    fn video_mkv_with_quality_cap() {
        let a = build_format_args(&DownloadOpts {
            max_height: Some(720),
            container: Container::Mkv,
            ..Default::default()
        });
        assert!(has_pair(&a, "-f", "bv*[height<=720]+ba/b[height<=720]"));
        assert!(has_pair(&a, "--merge-output-format", "mkv"));
    }

    #[test]
    fn audio_m4a_with_quality() {
        let a = build_format_args(&DownloadOpts {
            audio: true,
            audio_fmt: AudioFmt::M4a,
            audio_quality: Some(0),
            ..Default::default()
        });
        assert!(a.iter().any(|s| s == "-x"));
        assert!(has_pair(&a, "--audio-format", "m4a"));
        assert!(has_pair(&a, "--audio-quality", "0"));
        // เสียงต้องไม่มี container/subtitle
        assert!(!a.iter().any(|s| s == "--merge-output-format"));
        assert!(!a.iter().any(|s| s == "--embed-subs"));
    }

    #[test]
    fn subs_only_on_video_and_when_langs_present() {
        let with = build_format_args(&DownloadOpts {
            subs: true,
            sub_langs: "en,th".into(),
            ..Default::default()
        });
        assert!(has_pair(&with, "--sub-langs", "en,th"));
        assert!(with.iter().any(|s| s == "--embed-subs"));

        // subs=true แต่ภาษาว่าง → ข้าม
        let empty = build_format_args(&DownloadOpts {
            subs: true,
            sub_langs: "  ".into(),
            ..Default::default()
        });
        assert!(!empty.iter().any(|s| s == "--embed-subs"));

        // โหมดเสียงไม่ฝังซับแม้ subs=true
        let audio = build_format_args(&DownloadOpts {
            audio: true,
            subs: true,
            sub_langs: "en".into(),
            ..Default::default()
        });
        assert!(!audio.iter().any(|s| s == "--embed-subs"));
    }

    #[test]
    fn parse_helpers_fall_back_to_default() {
        assert_eq!(Container::parse("mkv"), Container::Mkv);
        assert_eq!(Container::parse("webm"), Container::Mp4);
        assert_eq!(AudioFmt::parse("OGG"), AudioFmt::Ogg);
        assert_eq!(AudioFmt::parse("flac"), AudioFmt::Mp3);
    }

    #[test]
    fn friendly_error_maps_common_cases() {
        assert!(friendly_error("ERROR: [udemy:course] course: HTTP Error 403: Forbidden").contains("Udemy"));
        assert!(friendly_error("ERROR: This video is DRM protected").contains("DRM"));
        assert!(friendly_error("ERROR: HTTP Error 403: Forbidden").contains("403"));
        assert!(friendly_error("ERROR: Please sign in to view").contains("ล็อกอิน"));
        assert!(friendly_error("ERROR: [youtube] Y3HfV4IroCU: This live event will begin in 24 hours.").contains("ยังไม่เริ่ม"));
        assert!(friendly_error("ERROR: [youtube] abc: Premieres in 3 minutes").contains("ยังไม่เริ่ม"));
        assert!(friendly_error("ERROR: [youtube] abc: This live stream recording is not available.").contains("ย้อนหลัง"));
        assert!(friendly_error("ERROR: [youtube] oQB8lYUZtrY: Requested format is not available. Use --list-formats for a list of available formats").contains("deno"));
        assert!(friendly_error("").contains("ไม่สำเร็จ"));
        // error อื่นๆ คงข้อความดิบไว้ (ตัด ERROR: ออก)
        assert!(friendly_error("ERROR: Video unavailable").contains("Video unavailable"));
    }

    #[test]
    fn cookies_build_and_args() {
        assert!(matches!(Cookies::from(None, None), Cookies::None));
        assert!(matches!(Cookies::from(Some("  ".into()), None), Cookies::None));
        assert!(cookie_args(&Cookies::None).is_empty());
        assert_eq!(
            cookie_args(&Cookies::from(Some("chrome".into()), None)),
            vec!["--cookies-from-browser", "chrome"]
        );
        // browser ชนะ file ถ้าใส่มาทั้งคู่
        assert_eq!(
            cookie_args(&Cookies::from(Some("safari".into()), Some("/x/c.txt".into()))),
            vec!["--cookies-from-browser", "safari"]
        );
        assert_eq!(
            cookie_args(&Cookies::from(None, Some("/x/c.txt".into()))),
            vec!["--cookies", "/x/c.txt"]
        );
    }

    #[test]
    fn wait_for_video_args_builds() {
        assert!(wait_for_video_args(None).is_empty());
        assert_eq!(wait_for_video_args(Some(60)), vec!["--wait-for-video", "60"]);
        // default DownloadOpts ไม่รอ
        assert!(DownloadOpts::default().wait_for_video.is_none());
    }
}
