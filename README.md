# vdo-dl

พอร์ตของ `~/.claude/skills/vdo-archive/scripts/vdo` มาเป็น Rust binary ตัวเดียว —
build ลง macOS / Linux / **Windows** ได้ เป็น **zero-dependency** (ไม่มี crate ภายนอก).
มี GUI mockup (ลอกโทน [codemunha.com](https://codemunha.com)) ที่ `ui/mockup.html`.

ตัว `vdo-dl` ทำหน้าที่สั่งงาน `yt-dlp` + `ffmpeg`:
โหลดคุณภาพสูงสุด (`bv*+ba/b`) → merge mp4 แบบ `-c copy` (ไม่ re-encode) →
verify ด้วย ffprobe → จัดเข้า `~/VDO/<หมวด>/`.

**ไม่ต้องลง yt-dlp/ffmpeg เอง** — ครั้งแรกที่รัน ถ้าเครื่องไม่มี vdo-dl จะโหลดมาเก็บเองที่
`<data>/vdo-dl/bin/` (Windows: `%LOCALAPPDATA%\vdo-dl\bin`) แล้วใช้ตลอด. ถ้าเครื่องมีอยู่ใน
PATH แล้ว (เช่น `brew install` บน mac) ก็ใช้ตัวนั้นเลย ไม่โหลดซ้ำ. โหลดผ่าน `curl` +
แตก zip ด้วย `tar` ที่มากับ Windows 10+/mac อยู่แล้ว.

| tool | จาก |
|------|-----|
| yt-dlp | GitHub release (`yt-dlp.exe` / `_macos` / `_linux`) |
| ffmpeg+ffprobe (Windows) | gyan.dev essentials (~40MB) → fallback BtbN GitHub CDN |
| ffmpeg (mac/Linux) | ใช้ PATH; mac ที่พังลองซ่อมด้วย `brew reinstall` |

อัปเดตเครื่องมือที่ bundle ไว้: `vdo-dl update` (yt-dlp รองรับ self-update `-U`).

## Usage

```
vdo-dl <URL>                      โหลด → พักที่ ~/VDO/tmp/ (พิมพ์ path)
vdo-dl <URL> "ชื่อเรื่อง"          โหลด + ตั้งชื่อ → ~/VDO/ยังไม่จัดหมวด/
vdo-dl <URL> "ชื่อเรื่อง" "หมวด"   โหลด + ตั้งชื่อ → ~/VDO/<หมวด>/
vdo-dl -F <URL>                   ดูตาราง format ที่มี (ไม่โหลด)
```

Env: `VDO_ROOT` (default `~/VDO`; บน Windows = `%USERPROFILE%\VDO`), `NO_COLOR`

## Build

### เครื่องตัวเอง (mac/Linux/Windows native)
```
cargo build --release          # ได้ target/release/vdo-dl  (vdo-dl.exe บน Windows)
```

### Cross-build mac → Windows
ต้องมี linker mingw-w64 + target:
```
brew install mingw-w64
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
# → target/x86_64-pc-windows-gnu/release/vdo-dl.exe
```
(`.cargo/config.toml` ตั้ง linker = `x86_64-w64-mingw32-gcc` ให้แล้ว)

## รันบน Windows
แค่วาง `vdo-dl.exe` ที่ไหนก็ได้แล้วรัน — ครั้งแรกมันจะโหลด yt-dlp + ffmpeg มาเก็บที่
`%LOCALAPPDATA%\vdo-dl\bin` เอง (ต้องมีเน็ตครั้งแรก). ครั้งต่อๆ ไปใช้ของที่เก็บไว้ ไม่ต้องโหลดซ้ำ.
```
vdo-dl "<URL>" "ชื่อเรื่อง" "หมวด"
vdo-dl update                       # อัปเดต yt-dlp + ffmpeg ที่เก็บไว้
```
ต้องเป็น Windows 10 (1803+) ขึ้นไป เพราะใช้ `curl.exe` + `tar.exe` ที่มากับระบบ.
