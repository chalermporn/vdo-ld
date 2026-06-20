# ทางลัดงานบ่อย — ดู `make help`
.PHONY: help release release-dry build run check clippy

help:                ## แสดงคำสั่งที่มี
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-14s\033[0m %s\n",$$1,$$2}'

release:             ## ออก release: make release VERSION=0.1.4  (เลื่อน CHANGELOG + bump + tag + push)
	@test -n "$(VERSION)" || { echo "ใช้: make release VERSION=0.1.4"; exit 1; }
	./scripts/release.sh $(VERSION)

release-dry:         ## ลองดูว่าจะแก้อะไร (ไม่เขียน/ไม่ push): make release-dry VERSION=0.1.4
	@test -n "$(VERSION)" || { echo "ใช้: make release-dry VERSION=0.1.4"; exit 1; }
	./scripts/release.sh $(VERSION) --dry-run

build:               ## build CLI (release)
	cargo build --release --bin vdo-dl

check:               ## cargo check ทั้ง workspace
	cargo check

clippy:              ## lint
	cargo clippy --all-targets
