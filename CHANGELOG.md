# Changelog

บันทึกการเปลี่ยนแปลงของ `vdo-dl` ทุกเวอร์ชันที่สำคัญ.
รูปแบบอิงตาม [Keep a Changelog](https://keepachangelog.com/) และใช้ [Semantic Versioning](https://semver.org/).

วิธีออกเวอร์ชันใหม่: ย้ายรายการจาก `[Unreleased]` ขึ้นหัวข้อเวอร์ชัน + ใส่วันที่ แล้ว
`git tag -a vX.Y.Z -m "..." && git push origin vX.Y.Z` → GitHub Actions build + ปล่อย release ครบทุก OS เอง.

## [Unreleased] / v0.1.1

ยังไม่มีการเปลี่ยนแปลงหลัง v0.1.0 — เติมรายการใหม่ที่นี่ระหว่างพัฒนา.

### Added
- _(ของใหม่)_

### Changed
- _(ปรับพฤติกรรม/ของเดิม)_

### Fixed
- _(แก้บั๊ก)_

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
- ยังไม่ได้ code-sign — macOS/Windows เตือน unknown publisher (mac: right-click → Open; Win: Run anyway).
- ไม่รองรับเนื้อหา DRM/auth-bypass (เช่น Udemy Business — โหลดไม่ได้ถาวร).
- เครื่องที่ไม่มี `sqlite3` → ข้าม index เงียบ ๆ (best-effort) ไม่กระทบการโหลด.

[Unreleased]: https://github.com/chalermporn/vdo-ld/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/chalermporn/vdo-ld/releases/tag/v0.1.0
