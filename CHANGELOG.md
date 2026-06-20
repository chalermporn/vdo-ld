# Changelog

บันทึกการเปลี่ยนแปลงของ `vdo-dl` ทุกเวอร์ชันที่สำคัญ.
รูปแบบอิงตาม [Keep a Changelog](https://keepachangelog.com/) และใช้ [Semantic Versioning](https://semver.org/).

วิธีออกเวอร์ชันใหม่: ย้ายรายการจาก `[Unreleased]` ขึ้นหัวข้อเวอร์ชัน **ในรูปแบบ `## [X.Y.Z] - <วันที่>`** แล้ว
`git tag -a vX.Y.Z -m "..." && git push origin vX.Y.Z` → GitHub Actions build + ปล่อย release ครบทุก OS เอง
(ดึง section นี้เป็น release notes ให้อัตโนมัติ).

> ⚠️ ถ้า tag `vX.Y.Z` ทั้งที่ยังไม่มีหัวข้อ `## [X.Y.Z]` ตรงกัน → workflow (`changelog-check`) จะ **fail ก่อน build**
> เพื่อกันลืมเปลี่ยน header. แก้หัวข้อให้ตรงแล้ว push tag ใหม่.

## [Unreleased]

เติมรายการใหม่ที่นี่ระหว่างพัฒนา.

### Added
- _(ของใหม่)_

### Changed
- _(ปรับพฤติกรรม/ของเดิม)_

### Fixed
- _(แก้บั๊ก)_

## [0.1.3] - 2026-06-21

### Fixed
- **GUI: ลูก playlist ขึ้น "ข้าม" ทั้งที่โหลดสำเร็จ** — คลิปเดี่ยวที่โหลดแบบเปิด "ทั้ง playlist"
  ทำให้ yt-dlp คืน `playlist_index=NA` → index ว่าง → GUI map แถวลูกไม่เจอ → โดน mark ข้าม.
  แก้ด้วย `childOf()` ที่ fallback ไปลูกตัวแรกที่ยังไม่เสร็จเมื่อ index ว่าง (yt-dlp ส่ง item ตามลำดับ).

### Added
- **ปุ่ม "เปิดต้นทาง" (↗) บนแถวที่โหลด** — ส่ง URL ต้นทาง (`webpage_url`) มากับ event
  เก็บที่ row → กดเปิดลิงก์ที่โหลดมาในเบราว์เซอร์ได้เลย (รู้ว่าโหลดมาจากไหน).
  _(provenance ยังบันทึก source_url + webpage_url ลง index ทุกครั้งเหมือนเดิม)._

## [0.1.2] - 2026-06-21

### Added
- **GUI: พรีวิวก่อนโหลด** — วางลิงก์ (หรือลากมาวาง) ในโหมด Smart → เด้ง popup แสดง
  **ชื่อ / ผู้ลง / ความยาว / thumbnail** + ตัวเลือกที่จะใช้ (วิดีโอ/เสียง·คุณภาพ·หมวด)
  ให้ตรวจก่อนกด "โหลดเลย". `probe_meta` ดึงเพิ่ม uploader + duration.
- **CI scaffold เซ็นโค้ด (opt-in via secrets)** — macOS notarize (`APPLE_*`) และ
  Windows code-sign (`WINDOWS_CERTIFICATE*`); ไม่มี secrets ก็ build แบบไม่เซ็นตามเดิม.
- README badges (release / downloads / platforms).

### Changed
- popup พรีวิวเด้ง **ทันที** ตอนกดวางลิงก์ — อ่าน clipboard (`pbpaste`/`powershell`)
  + ดึงข้อมูลข้างใน popup แทนที่จะรอก่อนเด้ง (เดิมมี gap จากการ spawn process).
- Smart Mode: จาก "วางแล้วโหลดทันที" → "วางแล้วเด้งพรีวิวให้ยืนยัน".

### Fixed
- เอกสาร macOS "is damaged" — ใช้ `xattr -dr com.apple.quarantine` (right-click→Open แก้เคสนี้ไม่ได้).

## [0.1.1] - 2026-06-20

ปรับเฉพาะ **release tooling / เอกสาร** — ตัวโปรแกรม (CLI + GUI binary) **เหมือน v0.1.0 ทุกประการ**
(ไม่มีการแก้ `src/`), ไม่ต้องอัปเดตถ้าใช้ v0.1.0 อยู่แล้ว.

### Added
- **`CHANGELOG.md`** (รูปแบบ Keep a Changelog) — บันทึกการเปลี่ยนแปลงรายเวอร์ชัน.
- **release notes อัตโนมัติจาก CHANGELOG** — job `notes` ดึง section `## [X.Y.Z]` มาเป็น body ของ
  GitHub Release (ต่อท้ายด้วยวิธีติดตั้ง/asset legend) แทนข้อความ fix ไว้.
- **`changelog-check`** — job ตรวจก่อน build: ถ้า tag ไม่มี section ตรงกันใน CHANGELOG จะ
  **fail ก่อน build** (กันลืมเปลี่ยน header, ไม่เปลือง build ทุก OS) พร้อม error annotation บอกวิธีแก้.

## [0.1.0] - 2026-06-20

รีลีสแรก — core เดียว (zero-dependency, ใช้แต่ `std`) สองหน้าตา: CLI + GUI (Tauri v2),
ปล่อย binary ครบทุก OS ผ่าน GitHub Actions.

### Added
- **โหลดวิดีโอคุณภาพสูงสุด** — `yt-dlp` `bv*+ba/b` → merge `.mp4` แบบ `-c copy` (ไม่ re-encode) →
  verify ด้วย `ffprobe` → จัดเข้า `~/VDO/<หมวด>/<ชื่อ>.<ext>`.
- **เตรียมเครื่องมือเอง** (`ensure_tools`) — หา yt-dlp/ffmpeg จาก managed bin → PATH → โหลดมาเอง
  (`curl` + แตก zip ด้วย `tar` ที่มากับ OS); ไม่ต้องลงเอง.
- **ตัวเลือก CLI**: `--audio` (→ mp3/m4a/ogg), `--quality N`, `--mkv`, `--audio-quality`,
  `--subs[=LANGS]`, `--playlist` (→ โฟลเดอร์ย่อย + `.m3u`), `--wait-for-video[=N]`,
  `--cookies-from-browser`, `--cookies FILE`, `-F` (ดู format), `update`.
- **GUI (Tauri v2)** โทน codemunha — วางลิงก์/อ่าน clipboard, Smart Mode, เลือกวิดีโอ/เสียง + คุณภาพ,
  progress แยกต่อรายการ (โหลดพร้อมกันได้), หยุด/ลองใหม่, เปิดโฟลเดอร์เมื่อเสร็จ, ดึงชื่อ+thumbnail,
  คุกกี้เบราว์เซอร์/ไฟล์, dark/light, อัปเดตเครื่องมือในตั้งค่า.
- **Provenance index (SQLite)** — ทุกครั้งที่โหลดสำเร็จ บันทึก source URL + metadata
  (ชื่อ, ผู้ลง, วันอัปโหลด, ความยาว, แหล่งที่มา, ความละเอียด, ขนาด ฯลฯ) ลง `~/VDO/.vdo-dl/index.db`
  ผ่าน `sqlite3` ที่มากับ OS (คง zero-dep). ดึง metadata ฟรีตอนโหลด ไม่เรียก yt-dlp ซ้ำ.
- **ค้นหาประวัติ** — `vdo-dl search "คำค้น"` (CLI) + แท็บ **ประวัติ** ใน GUI; ค้นจาก
  ชื่อ/ผู้ลง/หมวด/URL/แหล่งที่มา, เปิดต้นทาง/เปิดโฟลเดอร์/ลบได้.
- **`vdo-dl backfill`** — สแกนไฟล์เก่าใน `~/VDO/` เข้า index (ของเก่าไม่มี source URL).
- **ยกเลิกการโหลด** — watcher thread + `AtomicBool` ต่อ download id; รองรับโหลดหลายอันพร้อมกัน.
- **ความปลอดภัย path** — `delete_file` ลบได้เฉพาะไฟล์ใต้ `vdo_root()`; `sanitize_title` กันอักขระต้องห้าม;
  `sql_quote` กัน SQL injection.
- **CI/CD ครบทุก OS** — `.github/workflows/release.yml` (matrix) build บน runner จริงของแต่ละ OS,
  push tag `v*` → GitHub Release เดียว แนบ GUI (`.dmg` aarch64/x64 · `.AppImage`/`.deb`/`.rpm` ·
  `*-setup.exe`/`.msi`) + CLI ของทุก os/arch.

### Known limitations
- ยังไม่ได้ code-sign — macOS ขึ้น "is damaged" (แก้: `xattr -dr com.apple.quarantine <app>`); Windows SmartScreen เตือน (Run anyway).
- ไม่รองรับเนื้อหา DRM/auth-bypass (เช่น Udemy Business — โหลดไม่ได้ถาวร).
- เครื่องที่ไม่มี `sqlite3` → ข้าม index เงียบ ๆ (best-effort) ไม่กระทบการโหลด.

[Unreleased]: https://github.com/chalermporn/vdo-ld/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/chalermporn/vdo-ld/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/chalermporn/vdo-ld/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/chalermporn/vdo-ld/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/chalermporn/vdo-ld/releases/tag/v0.1.0
