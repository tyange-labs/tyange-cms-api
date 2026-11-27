#!/usr/bin/env bash
# 로컬에서 MCP 커넥터 도구를 Claude Code로 직접 써 본다. OAuth·패스키는 건너뛰고 토큰을 로컬 DB에 바로 넣는다.
#
#   1) scripts/mcp-local.sh serve    # 로컬 서버(다른 터미널에 띄워 둔다)
#   2) scripts/mcp-local.sh token    # 토큰을 만들고 Claude Code에 붙이는 명령을 보여 준다
#   3) 출력된 `claude mcp add …` 실행 → Claude Code에서 "글 목록 보여 줘" 등
#   끝나면 `claude mcp remove tyange-local`. 데이터는 ./data/mcp-local (git 제외)에만 쌓인다.
#
# 운영 서비스에 닿지 않게 블로그 재배포·패스키·수집 작업은 끈다(.env가 있어도 아래 값이 이긴다).
set -euo pipefail

PORT="${MCP_LOCAL_PORT:-18081}"
DIR="${MCP_LOCAL_DIR:-./data/mcp-local}"
URL="http://localhost:${PORT}"
CLIENT_ID="tyange-claude"

case "${1:-}" in
  serve)
    mkdir -p "$DIR"
    exec env \
      DATABASE_PATH="$DIR/db.sqlite" \
      UPLOAD_PATH="$DIR/images" \
      LISTEN_ADDR="127.0.0.1:${PORT}" \
      JWT_ACCESS_SECRET=local-dev-only \
      TYANGE_BLOG_REDEPLOY_TOKEN= \
      PASSKEY_RP_ID= \
      COVER_FETCH_ENABLED=false \
      YES24_FETCH_ENABLED=false \
      GOTOLIB_PUSH_ENABLED=false \
      VAPID_KEY_PATH="$DIR/vapid_private.key" \
      COVERS_DIR="$DIR/covers" \
      MCP_PUBLIC_URL="$URL" \
      MCP_OAUTH_CLIENT_ID="$CLIENT_ID" \
      MCP_IMAGE_BASE_URL="$URL" \
      cargo run
    ;;
  token)
    DB="$DIR/db.sqlite"
    if [ ! -f "$DB" ]; then
      echo "DB가 없습니다. 먼저 'scripts/mcp-local.sh serve'로 서버를 한 번 띄우세요." >&2
      exit 1
    fi
    rand() { uuidgen | tr -d '-' | tr '[:upper:]' '[:lower:]'; }
    token="mcpa_$(rand)$(rand)"
    hash="$(printf '%s' "$token" | shasum -a 256 | cut -d' ' -f1)"
    expires_at=$(( $(date +%s) + 30 * 24 * 60 * 60 ))
    sqlite3 "$DB" <<SQL
INSERT OR IGNORE INTO users (user_id, user_role) VALUES ('local-admin', 'admin');
INSERT INTO mcp_oauth_tokens (token_hash, kind, user_id, client_id, resource, expires_at)
VALUES ('$hash', 'access', 'local-admin', '$CLIENT_ID', '$URL/mcp', $expires_at);
SQL
    echo "30일짜리 로컬 토큰을 만들었습니다(사용자 local-admin). Claude Code에 붙이려면:"
    echo
    echo "  claude mcp add --transport http tyange-local $URL/mcp --header \"Authorization: Bearer $token\""
    ;;
  *)
    sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
