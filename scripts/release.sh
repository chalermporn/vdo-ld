#!/usr/bin/env bash
# scripts/release.sh — ออก release ใหม่ครบขั้นตอนในคำสั่งเดียว
#
#   ./scripts/release.sh 0.1.4               # ออก v0.1.4 (วันที่ = วันนี้)
#   ./scripts/release.sh 0.1.4 --date 2026-07-01
#   ./scripts/release.sh 0.1.4 --dry-run     # โชว์ว่าจะแก้อะไร ไม่เขียน/ไม่ push
#   ./scripts/release.sh 0.1.4 --no-push     # commit + tag ในเครื่อง แต่ไม่ push (กันพลาด)
#
# ทำอะไร (ตามลำดับ):
#   1) เลื่อน CHANGELOG: ## [Unreleased] → ## [X.Y.Z] - <วันที่> + ใส่ [Unreleased] เปล่าใหม่ + อัปเดต link refs
#      (ถ้ามี ## [X.Y.Z] อยู่แล้ว ใช้อันนั้นเลย ไม่เลื่อน)
#   2) bump version 0.1.x ใน Cargo.toml, src-tauri/Cargo.toml, src-tauri/tauri.conf.json + Cargo.lock
#   3) commit "Release vX.Y.Z" → tag vX.Y.Z → push main + tag
#   4) GitHub Actions (.github/workflows/release.yml) ทำต่อเอง: changelog-check → build ทุก OS → notes
#
# กฎ: section ของเวอร์ชันใน CHANGELOG ต้อง "มีเนื้อหาจริง" (ไม่ใช่ placeholder _(...)_ ล้วน)
#     ไม่งั้น abort — กันออก release ที่ notes ว่าง.
set -euo pipefail

# ── helpers ──────────────────────────────────────────────────────────────
c_red=$'\033[31m'; c_grn=$'\033[32m'; c_ylw=$'\033[33m'; c_dim=$'\033[2m'; c_off=$'\033[0m'
say()  { printf '%s▸%s %s\n' "$c_grn" "$c_off" "$*"; }
warn() { printf '%s!%s %s\n' "$c_ylw" "$c_off" "$*"; }
die()  { printf '%s✗ %s%s\n' "$c_red" "$*" "$c_off" >&2; exit 1; }

# ── args ─────────────────────────────────────────────────────────────────
VER=""; DATE=""; DRY=0; NO_PUSH=0
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --no-push) NO_PUSH=1 ;;
    --date)    DATE="${2:-}"; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    v*)        VER="${1#v}" ;;
    *)         VER="$1" ;;
  esac
  shift
done
[ -n "$VER" ] || die "ต้องระบุเวอร์ชัน เช่น: ./scripts/release.sh 0.1.4"
echo "$VER" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$' || die "เวอร์ชันต้องเป็น X.Y.Z (ได้: '$VER')"
[ -n "$DATE" ] || DATE="$(date +%F)"   # วันนี้ (YYYY-MM-DD)
TAG="v$VER"

# ── ไปที่ราก repo ────────────────────────────────────────────────────────
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ -f CHANGELOG.md ] && [ -f Cargo.toml ] || die "ไม่เจอ CHANGELOG.md / Cargo.toml — รันจากใน repo vdo-dl"
trap 'rm -f "$ROOT/CHANGELOG.md.tmp" "$ROOT/CHANGELOG.md.tmp2"' EXIT  # กันไฟล์ชั่วคราวค้างตอน abort

# ── preflight ────────────────────────────────────────────────────────────
git rev-parse --git-dir >/dev/null 2>&1 || die "ไม่ใช่ git repo"
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
[ "$BRANCH" = "main" ] || warn "อยู่ branch '$BRANCH' (ไม่ใช่ main) — release ปกติออกจาก main"
if [ "$DRY" -eq 0 ] && [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  die "working tree มีของค้าง — commit/stash ก่อนออก release (กันปนกับ commit release)"
fi
git rev-parse "$TAG" >/dev/null 2>&1 && die "tag $TAG มีอยู่แล้ว"

dlabel=""; [ "$DRY" -eq 1 ] && dlabel="  ${c_dim}[dry-run]${c_off}"
say "เตรียมออก ${c_grn}$TAG${c_off} (วันที่ $DATE)${dlabel}"

# ── 1) CHANGELOG ─────────────────────────────────────────────────────────
have_section() { grep -qE "^## \[$VER\]" CHANGELOG.md; }

promote_changelog() {
  # ดึงเวอร์ชันก่อนหน้าจาก link ref ของ [Unreleased] (…compare/vPREV...HEAD)
  local prev
  prev="$(grep -oE '^\[Unreleased\]:.*compare/v[0-9.]+\.\.\.HEAD' CHANGELOG.md | grep -oE 'v[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
  # เลื่อน [Unreleased] → [VER] + ใส่ [Unreleased] เปล่าใหม่; ตัด placeholder + ### ที่ว่าง
  awk -v ver="$VER" -v date="$DATE" '
    function emit_fresh() {
      print "## [Unreleased]"; print "";
      print "เติมรายการใหม่ที่นี่ระหว่างพัฒนา."; print "";
      print "### Added";   print "- _(ของใหม่)_"; print "";
      print "### Changed"; print "- _(ปรับพฤติกรรม/ของเดิม)_"; print "";
      print "### Fixed";   print "- _(แก้บั๊ก)_"; print "";
      print "## [" ver "] - " date;
    }
    function flush(   i,j,hdr,has,line) {
      # พิมพ์ buf[] โดยตัดบรรทัดเกริ่น/placeholder และ ### ที่ไม่มี bullet จริง
      print ""   # บรรทัดว่างหลัง "## [X.Y.Z] - date" ให้เข้าสไตล์เดิม
      i=0
      while (i<n) {
        line=buf[i]
        if (line ~ /^### /) {
          hdr=line; has=0; j=i+1
          while (j<n && buf[j] !~ /^### /) {
            if (buf[j] ~ /^- / && buf[j] !~ /^- _\(/) has=1
            j++
          }
          if (has) {
            print hdr
            # เก็บ bullet จริง + บรรทัดต่อ (wrapped) — ตัด placeholder/เกริ่น/บรรทัดว่าง
            for (k=i+1; k<j; k++)
              if (buf[k] !~ /^- _\(/ && buf[k] !~ /เติมรายการใหม่/ && buf[k] !~ /^[[:space:]]*$/) print buf[k]
            print ""
          }
          i=j
        } else { i++ }   # บรรทัดนอก ### (เกริ่น/ว่าง) — ข้าม
      }
    }
    $0 ~ /^## \[Unreleased\]/ && !done { emit_fresh(); inU=1; next }
    inU && /^## \[/ { inU=0; done=1; flush(); print $0; next }
    inU { buf[n++]=$0; next }
    { print }
  ' CHANGELOG.md > CHANGELOG.md.tmp

  # อัปเดต link refs ท้ายไฟล์: [Unreleased] เทียบจาก VER, เพิ่มบรรทัด [VER]
  if [ -n "$prev" ]; then
    awk -v ver="$VER" -v prev="$prev" '
      /^\[Unreleased\]:/ {
        sub(/compare\/v[0-9.]+\.\.\.HEAD/, "compare/v" ver "...HEAD")
        print
        print "[" ver "]: https://github.com/chalermporn/vdo-ld/compare/" prev "...v" ver
        next
      }
      { print }
    ' CHANGELOG.md.tmp > CHANGELOG.md.tmp2 && mv CHANGELOG.md.tmp2 CHANGELOG.md.tmp
  else
    warn "หา link ref [Unreleased] ไม่เจอ — ข้ามการอัปเดต compare links (เพิ่มเองได้)"
  fi
}

if have_section; then
  say "CHANGELOG มี ## [$VER] อยู่แล้ว — ใช้อันนั้น (ไม่เลื่อน [Unreleased])"
  CL_TARGET="CHANGELOG.md"
else
  say "เลื่อน CHANGELOG: [Unreleased] → [$VER] - $DATE"
  promote_changelog
  CL_TARGET="CHANGELOG.md.tmp"
fi

# เช็คว่า section มี bullet จริง (กัน notes ว่าง) — เช็คบนไฟล์เป้าหมาย
n_real="$(awk -v ver="$VER" '
  $0 ~ "^## \\[" ver "\\]" {f=1; next}
  f && /^## / {exit}
  f && /^\[[^][]*\]:/ {exit}
  f && /^- / && $0 !~ /^- _\(/ {c++}
  END{print c+0}' "$CL_TARGET")"
[ "$n_real" -ge 1 ] || die "section [$VER] ไม่มีเนื้อหาจริง ($n_real bullet) — เขียนรายการใต้ ## [Unreleased] ก่อน แล้วรันใหม่"
say "section [$VER] มี $n_real รายการ ✓"

# ── 2) bump version ──────────────────────────────────────────────────────
bump_file() { # <file> <regex-เดิม> <ใหม่>
  local f="$1" old="$2" new="$3"
  grep -qE "$old" "$f" || die "ไม่เจอบรรทัด version ใน $f"
  if [ "$DRY" -eq 1 ]; then printf '  %s: ' "$f"; grep -E "$old" "$f" | head -1 | sed 's/^/→ /'
  else sed -i.bak -E "s|$old|$new|" "$f" && rm -f "$f.bak"; fi
}
say "bump version → $VER"
bump_file Cargo.toml                 '^version = "[0-9.]+"'   "version = \"$VER\""
bump_file src-tauri/Cargo.toml       '^version = "[0-9.]+"'   "version = \"$VER\""
bump_file src-tauri/tauri.conf.json  '"version": "[0-9.]+"'   "\"version\": \"$VER\""

# ── dry-run: โชว์ผลแล้วจบ ─────────────────────────────────────────────────
if [ "$DRY" -eq 1 ]; then
  echo; say "${c_dim}[dry-run]${c_off} preview CHANGELOG section [$VER]:"
  awk -v ver="$VER" '
    $0 ~ "^## \\[" ver "\\]" {f=1; print; next}
    f && /^## / {exit}
    f && /^\[[^][]*\]:/ {exit}
    f {print}
  ' "$CL_TARGET" | sed 's/^/  /' | head -30
  rm -f CHANGELOG.md.tmp
  echo; say "${c_dim}[dry-run] จบ — ไม่ได้เขียน/commit/push อะไร${c_off}"
  exit 0
fi

# ใช้ CHANGELOG ที่เลื่อนแล้ว (ถ้ามี)
[ -f CHANGELOG.md.tmp ] && mv CHANGELOG.md.tmp CHANGELOG.md

# ── regenerate Cargo.lock (อัปเดตเวอร์ชัน workspace member) ────────────────
say "อัปเดต Cargo.lock"
cargo check --quiet 2>/dev/null || cargo check --quiet || die "cargo check ล้มเหลว"

# ── 3) commit + tag + push ───────────────────────────────────────────────
say "commit + tag $TAG"
git add CHANGELOG.md Cargo.toml Cargo.lock src-tauri/Cargo.toml src-tauri/tauri.conf.json
git commit -m "Release $TAG

bump → $VER + finalize CHANGELOG [$VER].

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>" -q
git tag -a "$TAG" -m "vdo-dl $TAG"

if [ "$NO_PUSH" -eq 1 ]; then
  warn "--no-push: commit + tag เสร็จในเครื่องแล้ว ยังไม่ push"
  say  "push เองด้วย: git push origin main && git push origin $TAG"
  exit 0
fi
say "push main + $TAG"
git push origin main -q
git push origin "$TAG" -q

echo
say "${c_grn}ออก $TAG แล้ว!${c_off} GitHub Actions กำลัง build ทุก OS + ทำ release notes จาก CHANGELOG"
say "ดูสถานะ: ${c_dim}gh run watch --exit-status \$(gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')${c_off}"
say "release:  https://github.com/chalermporn/vdo-ld/releases/tag/$TAG"
