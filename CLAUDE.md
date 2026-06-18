# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## ภาพรวม

`vdo-dl` คือ wrapper รอบ `yt-dlp` + `ffmpeg` ที่ส่งลิงก์วิดีโอเข้าไป → ได้ไฟล์คุณภาพสูงสุด → จัดเข้า `~/VDO/<หมวด>/`. เขียนด้วย Rust **zero-dependency สำหรับ core** (ไม่มี crate ภายนอกใน `src/lib.rs` — ใช้แต่ `std`). มี 2 หน้าตา: CLI binary และ Tauri GUI ที่เรียก core เดียวกัน.

โค้ดและคอมเมนต์ในเรพอนี้เป็น **ภาษาไทย** — เขียนของใหม่ให้เข้าชุดเดิม.

## สถาปัตยกรรม — core เดียว, 2 หน้าตา

หัวใจคือ **`src/lib.rs` (crate `vdo_dl`)** ที่ทั้ง CLI และ GUI พึ่งพา. กฎออกแบบที่ต้องรักษาไว้:

- **ทุกฟังก์ชัน public คืน `VResult<T>` (= `Result<T, String>`) — ไม่มี `die`/`exit`/`panic` ใน core.** การ exit อยู่ที่ชั้น CLI (`src/main.rs::die`) เท่านั้น เพื่อให้ GUI จัดการ error ได้โดยไม่ crash.
- **สถานะ/ความคืบหน้าออกผ่าน callback ที่ inject เข้าไป** ไม่ใช่ print ตรงๆ:
  - `Log = dyn Fn(&str)` — ข้อความสถานะ (เช่น "โหลด yt-dlp ครั้งแรก…")
  - `Progress = dyn Fn(f32, &str)` — `(percent 0..100, บรรทัดดิบ)`; `percent < 0` = บรรทัดสถานะเฉยๆ ไม่ใช่ progress
  - CLI map → พิมพ์ stderr; Tauri map → emit เป็น event. **เพิ่มฟีเจอร์ใหม่ใน core ต้องส่งผลผ่าน callback เสมอ ห้าม print ใน lib.rs.**

Flow หลัก (เรียงตามขั้นใน `download_video` ของ Tauri / `run` ของ CLI):
`ensure_tools` (โหลด yt-dlp/ffmpeg ถ้าขาด) → `download` (yt-dlp `bv*+ba/b` → merge mp4 `-c copy` ไม่ re-encode) → `verify` (ffprobe ดึง w/h/codec/size) → `file_into` (ย้ายเข้า `~/VDO/<หมวด>/<title>.<ext>`).

### Tool provisioning (`ensure_tools`)
ลำดับการหา tool: **managed bin (`bin_dir()`) ก่อน → PATH → โหลดมาเอง**. โหลดผ่าน `curl`, แตก zip ด้วย `tar` ที่มากับ OS (Windows 10 1803+/mac มีให้). `bin_dir()` ต่าง OS: Windows `%LOCALAPPDATA%\vdo-dl\bin`, mac `~/Library/Application Support/vdo-dl/bin`, Linux `$XDG_DATA_HOME/vdo-dl/bin`. ffmpeg: Windows โหลด bundle (gyan.dev → fallback BtbN); mac ลอง `brew reinstall`; Linux บอกให้ลงเอง.

### การยกเลิก (cancel)
`download` รับ `&AtomicBool` — watcher thread ใน `std::thread::scope` คอย poll แล้ว `kill_pid` ตัว yt-dlp เมื่อ flag เป็น true. ฝั่ง Tauri เก็บ flag ต่อ download id ใน `Jobs(Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>)`; `cancel_download(id)` สั่ง store true. **ทุก event ฝั่ง Tauri แนบ `id`** เพื่อ map กลับ row ที่ถูกต้อง (รองรับโหลดพร้อมกันหลายอัน) — รักษา invariant นี้เมื่อเพิ่ม command/event.

### ความปลอดภัยของ path
`delete_file` ลบได้เฉพาะไฟล์ที่ `canonicalize` แล้วอยู่ใต้ `vdo_root()` เท่านั้น. `sanitize_title` ล้างอักขระตั้งชื่อไฟล์ไม่ได้ (สำคัญบน Windows). อย่าถอดยามเหล่านี้ออก.

## โครงไฟล์

```
src/lib.rs        core ทั้งหมด (crate vdo_dl) — zero-dep, callback-based, Result ทุกฟังก์ชัน
src/main.rs       CLI บางๆ ครอบ core (จุดเดียวที่ exit/print สี ANSI)
src-tauri/src/lib.rs   Tauri commands + events ครอบ core เดียวกัน (app_lib::run)
src-tauri/src/main.rs  entry point เรียก app_lib::run()
ui/index.html     frontend จริง — เรียก window.__TAURI__.core.invoke + ฟัง event "vdo://progress" | "vdo://status"
ui/mockup.html    ดีไซน์อ้างอิงเฉยๆ (ไม่ผูกกับโค้ด)
```

เป็น Cargo workspace: root crate `vdo-dl` (lib + bin) + member `src-tauri` (crate `vdo-dl-app`, lib name `app_lib`) ที่ `vdo-dl = { path = ".." }`.

## คำสั่งที่ใช้บ่อย

```bash
# CLI
cargo build --release                    # → target/release/vdo-dl
cargo run -- <URL> "ชื่อ" "หมวด"          # รัน CLI ตรงๆ
cargo run -- -F <URL>                    # ดูตาราง format (ไม่โหลด)
cargo run -- --audio <URL> "ชื่อ" "เพลง"  # เสียงอย่างเดียว → mp3
cargo run -- --quality 720 <URL>         # จำกัด ≤720p
cargo run -- update                      # อัปเดต yt-dlp (+ ffmpeg บน Windows)

cargo check                              # ตรวจเร็วทั้ง workspace
cargo clippy --all-targets               # lint

# GUI (Tauri v2) — ต้องมี bun
bunx @tauri-apps/cli@2 dev               # หน้าต่าง dev + hot reload
bunx @tauri-apps/cli@2 build             # release: .app (mac) / .exe+installer (Win)

# Cross-build mac → Windows CLI (.cargo/config.toml ตั้ง linker ให้แล้ว)
brew install mingw-w64 && rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu   # → ...release/vdo-dl.exe
```

> ยังไม่มี test suite ในเรพอ. GUI cross-build จาก mac ไป Windows ยุ่ง — build บน Windows runner (GitHub Actions) แทน; ตัว CLI cross-build ไป `.exe` จาก mac ได้ปกติ.

## Env vars
- `VDO_ROOT` — โฟลเดอร์ปลายทาง (default `~/VDO`, Windows `%USERPROFILE%\VDO`)
- `VDO_BIN` — override ที่เก็บ yt-dlp/ffmpeg ที่โหลดมาเอง
- `NO_COLOR` — ปิดสี ANSI ใน CLI

## เพิ่ม Tauri command ใหม่ (checklist)
1. เพิ่ม logic ใน `src/lib.rs` แบบคืน `VResult` + รับ callback ถ้ามีสถานะ
2. ห่อเป็น `#[tauri::command]` ใน `src-tauri/src/lib.rs` (ถ้า blocking ใช้ `spawn_blocking`; แนบ `id` ใน event)
3. เพิ่มชื่อใน `tauri::generate_handler![...]`
4. เรียกจาก `ui/index.html` ผ่าน `invoke("ชื่อ_command", {...})`
