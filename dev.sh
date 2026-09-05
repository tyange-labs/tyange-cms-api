#!/usr/bin/env bash
# 로컬 개발용 원커맨드 실행 스크립트.
# .env가 없으면 개발 전용 더미 값으로 하나 만들고 서버를 띈다.
#
# 안 채우는 값(GOOGLE_CLIENT_ID, VAPID_*)은 기능 자체가 우아하게 꺼지는 값들이라
# 로컬 CMS 개발엔 필요 없다 — Google 로그인은 CMS가 이제 안 부르고,
# GET /push/public-key는 VAPID_PUBLIC_KEY가 없으면 503을 낼 뿐 서버가 안 죽는다.
#
# 이미 .env가 있으면 손대지 않는다 — 일부 값만 채워주는 식으로 손대면
# 사람이 의도적으로 넣어둔 값을 건드릴 위험이 있어서, 아예 없을 때만 만든다.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

if [ ! -f .env ]; then
  echo "[dev.sh] .env가 없어서 개발용으로 하나 만듭니다 (JWT 시크릿은 랜덤 생성, 프로덕션에는 쓰지 말 것)"
  cat > .env <<EOF
DATABASE_PATH=./data/database.db
UPLOAD_PATH=.uploads/images
UPLOAD_MAX_BYTES=20971520

JWT_ACCESS_SECRET=$(openssl rand -hex 32)
JWT_REFRESH_SECRET=$(openssl rand -hex 32)
EOF
  echo "[dev.sh] .env 생성 완료"
fi

echo "[dev.sh] cargo run -q (0.0.0.0:8080)"
exec cargo run -q
