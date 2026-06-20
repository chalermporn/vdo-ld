# vdo-dl

[![latest release](https://img.shields.io/github/v/release/chalermporn/vdo-ld?sort=semver&label=release)](https://github.com/chalermporn/vdo-ld/releases/latest)
[![downloads](https://img.shields.io/github/downloads/chalermporn/vdo-ld/total?label=downloads)](https://github.com/chalermporn/vdo-ld/releases)
[![platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-blue)](https://github.com/chalermporn/vdo-ld/releases/latest)

> ⬇️ โหลดตัวติดตั้ง/ไบนารีล่าสุดได้ที่ [**Releases**](https://github.com/chalermporn/vdo-ld/releases/latest) — มีครบทุก OS.

พอร์ตของ `~/.claude/skills/vdo-archive/scripts/vdo` มาเป็น Rust binary ตัวเดียว —
build ลง macOS / Linux / **Windows** ได้ เป็น **zero-dependency** (ไม่มี crate ภายนอก).

มาพร้อม **GUI (Tauri)** โทน [codemunha.com](https://codemunha.com) ที่เรียก core เดียวกับ CLI.
ทุกครั้งที่โหลดยัง **จดประวัติ (source URL + metadata) ลง SQLite** ให้ค้นย้อนหลังได้ทั้ง CLI และ GUI.

## โครงสร้าง
```
src/lib.rs        core: download/provision/verify/จัดหมวด (Result + progress callback, zero-dep)
src/main.rs       CLI บางๆ ครอบ core
src-tauri/        Tauri v2 app — command + event ครอบ core เดียวกัน
ui/index.html     frontend (เรียก IPC จริง; เปิดในเบราว์เซอร์ = เดโม)
ui/mockup.html    ดีไซน์อ้างอิงเฉย ๆ
```

## GUI (Tauri)
```
bunx @tauri-apps/cli@2 dev      # dev: เปิดหน้าต่าง + hot reload
bunx @tauri-apps/cli@2 build    # release: ได้ .app (mac) / .exe+installer (Windows)
```
> Windows: cross-build GUI จาก mac ยุ่ง — แนะนำ build บน Windows runner (GitHub Actions).
> ตัว CLI (`vdo-dl`) ยัง cross-build ไป `.exe` จาก mac ได้ปกติ (ดูล่าง).

### ⚠️ macOS เปิดครั้งแรกขึ้น "is damaged and can't be opened"
ไม่ใช่ไฟล์เสีย — แอป**ยังไม่ได้ notarize กับ Apple** พอโหลดผ่านเบราว์เซอร์ไฟล์ติด flag `com.apple.quarantine`
macOS เลยบล็อกแล้วขึ้นข้อความหลอกว่า "damaged". ลบ quarantine แล้วเปิดได้ทันที:
```bash
# GUI (.app) — ลากเข้า /Applications ก่อน แล้ว:
xattr -dr com.apple.quarantine /Applications/vdo-dl.app
open /Applications/vdo-dl.app

# CLI binary:
xattr -dr com.apple.quarantine ~/Downloads/vdo-dl-macos-aarch64 && chmod +x ~/Downloads/vdo-dl-macos-aarch64
```
จะหายถาวร (ไม่ต้องทำขั้นนี้) ต่อเมื่อเซ็นด้วย **Apple Developer ID + notarize** — ต้องมีบัญชี Apple Developer ($99/ปี).

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

## Usage (CLI)

```
vdo-dl [options] <URL> ["ชื่อเรื่อง"] ["หมวด"]
vdo-dl -F <URL>                   ดูตาราง format ที่มี (ไม่โหลด)
vdo-dl update                     อัปเดต yt-dlp (+ ffmpeg บน Windows)
vdo-dl search ["คำค้น"]            ค้นประวัติที่เคยโหลด (ไม่ใส่คำ = ล่าสุด)
vdo-dl backfill                   สแกนไฟล์เก่าใน ~/VDO/ เข้า index

# ตัวอย่าง
vdo-dl <URL> "ชื่อ" "หมวด"                       วิดีโอคุณภาพสูงสุด → ~/VDO/<หมวด>/ชื่อ.mp4
vdo-dl --audio <URL> "ชื่อ" "เพลง"               เสียงอย่างเดียว → .mp3
vdo-dl --quality 720 <URL> "ชื่อ"                จำกัด ≤720p
vdo-dl --playlist <URL> "" "คอร์ส"               ทั้ง playlist → โฟลเดอร์ย่อย + .m3u
vdo-dl --cookies-from-browser chrome <URL> "ชื่อ" "หมวด"   เนื้อหาที่ต้องล็อกอิน
```

### Options

| Option | ความหมาย |
|--------|----------|
| `--audio` | โหลดเสียงอย่างเดียว |
| `--quality N` | จำกัดความสูงสูงสุด เช่น `1080`, `720` (ไม่ใส่ = สูงสุด) |
| `--mkv` | วิดีโอ: merge เป็น `.mkv` (default `.mp4`) |
| `--audio-format FMT` | เสียง: `mp3` \| `m4a` \| `ogg` (default `mp3`) |
| `--audio-quality N` | เสียง: `0` (ดีสุด) .. `10` |
| `--subs[=LANGS]` | ดาวน์โหลด + ฝังคำบรรยาย (ไม่ระบุ = `en,th`) |
| `--playlist` | โหลดทั้ง playlist → โฟลเดอร์ย่อย + `.m3u` |
| `--wait-for-video[=N]` | รอไลฟ์/พรีเมียร์ให้เริ่มก่อน (poll ทุก N วิ, default 60) |
| `--cookies-from-browser B` | ใช้คุกกี้จากเบราว์เซอร์ (`chrome`\|`safari`\|`firefox`\|`edge`\|`brave`\|…) |
| `--cookies FILE` | ใช้คุกกี้จากไฟล์ `cookies.txt` (Netscape) |

## GUI features
วางลิงก์ (อ่าน clipboard) · Smart Mode (วาง=โหลดทันที) · เลือกวิดีโอ/เสียง + คุณภาพ ·
แท็บ ทั้งหมด/วิดีโอ/เสียง/**ประวัติ** · progress แยกต่อรายการ (โหลดพร้อมกันได้) · หยุด/ลองใหม่ ·
เปิดโฟลเดอร์เมื่อเสร็จ · ดึงชื่อ+thumbnail อัตโนมัติ · จำรายการข้ามการเปิด/ปิด · ลากลิงก์มาวาง ·
คุกกี้ (เบราว์เซอร์/ไฟล์) · dark/light · อัปเดตเครื่องมือในตั้งค่า

**แท็บ ประวัติ** — ค้นจากชื่อ/ผู้ลง/หมวด/URL/แหล่งที่มา; แต่ละรายการกด เปิดต้นทาง (เบราว์เซอร์) ·
เปิดโฟลเดอร์ · ลบ (ลบไฟล์+ประวัติ หรือเอาออกจากประวัติเท่านั้น)

## ประวัติ / ค้นหา
ทุกครั้งที่โหลดสำเร็จ บันทึก source URL + metadata (ชื่อ, ผู้ลง, วันอัปโหลด, ความยาว, แหล่งที่มา,
ความละเอียด, ขนาด ฯลฯ) ลง DB กลางที่ `~/VDO/.vdo-dl/index.db` (SQLite). ใช้ `sqlite3` ที่มากับ OS
(เหมือนวิธีเรียก yt-dlp/ffmpeg) — core ยัง **zero-dependency**. ค้นได้ทั้งในแอป, ด้วย `vdo-dl search`,
หรือ SQL ตรง ๆ:
```
sqlite3 ~/VDO/.vdo-dl/index.db "SELECT title, uploader, source_url FROM downloads WHERE title LIKE '%react%';"
```
- ของเก่าที่โหลดก่อนมีฟีเจอร์นี้ → `vdo-dl backfill` ดึงเข้า index ได้ (แต่ source URL ของเก่ากู้ไม่ได้ — ไม่เคยเก็บ).
- **best-effort**: เครื่องไม่มี `sqlite3` (เช่น Windows ที่ไม่มี builtin) จะข้าม index เงียบ ๆ ไม่กระทบการโหลด.

Env: `VDO_ROOT` (default `~/VDO`; บน Windows = `%USERPROFILE%\VDO`), `VDO_BIN`, `NO_COLOR`

## Release (อัตโนมัติทุก OS)
push tag `v*` แล้ว GitHub Actions (`.github/workflows/release.yml`) จะ build บน runner จริงของแต่ละ OS
แล้วสร้าง **GitHub Release เดียว** แนบให้ครบ:
- GUI: macOS `.dmg` (Apple Silicon + Intel) · Linux `.AppImage`/`.deb` · Windows `*-setup.exe`/`.msi`
- CLI: `vdo-dl-<os>-<arch>` ของทุกแพลตฟอร์ม

**ออก release ด้วยสคริปต์เดียว** (เลื่อน CHANGELOG + bump version + commit + tag + push ให้ครบ):
```
# 1. เขียนรายการที่ทำใต้หัวข้อ ## [Unreleased] ใน CHANGELOG.md ก่อน
# 2. รัน (เลือกอย่างใดอย่างหนึ่ง):
make release VERSION=0.1.4            # หรือ
./scripts/release.sh 0.1.4
./scripts/release.sh 0.1.4 --dry-run # ลองดูว่าจะแก้อะไร ไม่เขียน/ไม่ push
```
สคริปต์จะเลื่อน `## [Unreleased]` → `## [0.1.4] - <วันนี้>` (ตัด placeholder/หัวข้อว่างให้), bump
เวอร์ชันใน 3 manifest + `Cargo.lock`, commit, tag, push → GitHub Actions build ทุก OS + ทำ notes ต่อเอง.
จะ abort ถ้า section ยังไม่มีเนื้อหาจริง (กัน release ที่ notes ว่าง).

หรือ tag เองตรง ๆ ก็ได้ (ต้องเลื่อน CHANGELOG header เป็น `## [X.Y.Z]` ก่อน):
```
git tag -a v0.1.0 -m "..." && git push origin v0.1.0    # → release ใหม่เด้งเอง
```
> กดรัน workflow เองได้ (workflow_dispatch) เพื่อ build เก็บเป็น artifact โดยไม่ปล่อย release.

### macOS notarization (CI)
ค่าเริ่มต้น build แบบ **adhoc** (ผู้ใช้ต้อง `xattr -dr com.apple.quarantine` ครั้งแรก — ดูหัวข้อ "is damaged" ด้านบน).
อยากให้หายขาด (ผู้ใช้เปิดได้เลย ไม่ต้องพิมพ์อะไร) ต้องมี **Apple Developer account ($99/ปี)** แล้วตั้ง GitHub Secrets 6 ตัว
(`Settings → Secrets and variables → Actions`). workflow ตรวจให้เอง — ไม่มี secrets ก็ยัง build adhoc ได้ปกติ:

| Secret | เอามาจากไหน |
|--------|-------------|
| `APPLE_CERTIFICATE` | export cert **"Developer ID Application"** จาก Keychain เป็น `.p12` → `base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | รหัสที่ตั้งตอน export `.p12` |
| `APPLE_SIGNING_IDENTITY` | ชื่อเต็มของ cert เช่น `Developer ID Application: ชื่อคุณ (TEAMID)` (ดูจาก `security find-identity -v -p codesigning`) |
| `APPLE_ID` | อีเมล Apple ID |
| `APPLE_PASSWORD` | **app-specific password** (สร้างที่ appleid.apple.com → Sign-In and Security → App-Specific Passwords) — ไม่ใช่รหัส Apple ID จริง |
| `APPLE_TEAM_ID` | Team ID 10 หลัก (ดูที่ developer.apple.com → Membership) |

ตั้งครบแล้ว tag เวอร์ชันใหม่ → bundle mac จะถูก sign (Developer ID + hardened runtime) แล้ว **notarize** อัตโนมัติ.

### Windows code-sign (CI)
ค่าเริ่มต้นไม่เซ็น → SmartScreen เตือน "unknown publisher" (ผู้ใช้กด More info → Run anyway).
อยากให้เซ็น ต้องมี **OV/EV code-signing cert** (จาก CA เช่น Sectigo/DigiCert, ~$200+/ปี) เป็นไฟล์ `.pfx`
แล้วตั้ง GitHub Secrets 2 ตัว — workflow import + เซ็นให้เอง (ไม่มี secrets ก็ build ไม่เซ็นตามเดิม):

| Secret | เอามาจากไหน |
|--------|-------------|
| `WINDOWS_CERTIFICATE` | ไฟล์ `.pfx` → base64: `base64 -i cert.pfx \| pbcopy` (mac) หรือ `[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))` (PowerShell) |
| `WINDOWS_CERTIFICATE_PASSWORD` | รหัสของไฟล์ `.pfx` |

ตั้งครบแล้ว tag เวอร์ชันใหม่ → `.exe`/`.msi`/`-setup.exe` ฝั่ง Windows จะถูกเซ็น (timestamp DigiCert, sha256) อัตโนมัติ.
> EV cert (hardware token) เซ็นใน CI ไม่ได้ตรง ๆ — ต้องใช้ cloud signing (เช่น Azure Trusted Signing); ถ้าใช้ EV บอกได้ จะปรับ workflow ให้.

## Build (เครื่องตัวเอง)

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
