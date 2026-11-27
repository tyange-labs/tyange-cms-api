#!/usr/bin/env bash
# MCP 커넥터 승인용 admin 패스키를 비밀번호 없이 등록하는 일회용 코드를 운영 서버 DB에 넣고 등록 주소를 보여 준다.
# SSH 접속(1Password 키) 자체가 본인 확인이다. 코드는 15분짜리이고 등록에 성공하면 지워진다.
#
#   scripts/mcp-setup-code.sh            # admin 'tyange'
#   scripts/mcp-setup-code.sh <user_id>  # 다른 admin
#
# 환경변수: MCP_SSH_HOST(기본 lightsail), MCP_PUBLIC_URL(기본 https://mcp.tyange.com),
#           MCP_SERVICE_DIR(기본 /home/ubuntu/to-the-world/deploy/tyange-cms-api — 거기 .env의 DATABASE_PATH를 쓴다)
set -euo pipefail

USER_ID="${1:-tyange}"
HOST="${MCP_SSH_HOST:-lightsail}"
PUBLIC_URL="${MCP_PUBLIC_URL:-https://mcp.tyange.com}"
SERVICE_DIR="${MCP_SERVICE_DIR:-/home/ubuntu/to-the-world/deploy/tyange-cms-api}"

# SQL에 그대로 넣으므로 안전한 글자만 받는다(계정 id는 이메일이나 영문 아이디).
if ! [[ "$USER_ID" =~ ^[A-Za-z0-9@._+-]+$ ]]; then
  echo "user_id에는 영문·숫자·@._+- 만 쓸 수 있습니다." >&2
  exit 1
fi

rand() { uuidgen | tr -d '-' | tr '[:upper:]' '[:lower:]'; }
code="mcps_$(rand)$(rand)"
hash="$(printf '%s' "$code" | shasum -a 256 | cut -d' ' -f1)"

# 코드 원문은 서버로 보내지 않는다(해시만). 사용자 확인·삽입은 한 번의 SSH로.
ssh "$HOST" "bash -s" <<REMOTE
set -euo pipefail
DB=\$(sudo grep -E '^DATABASE_PATH=' '$SERVICE_DIR/.env' | cut -d= -f2-)
role=\$(sudo sqlite3 "\$DB" "SELECT user_role FROM users WHERE user_id = '$USER_ID';")
if [ "\$role" != "admin" ]; then
  echo "'$USER_ID'은(는) admin 계정이 아닙니다(role: \${role:-없음})." >&2
  exit 1
fi
sudo sqlite3 "\$DB" "INSERT INTO mcp_passkey_setup_codes (code_hash, user_id, expires_at) VALUES ('$hash', '$USER_ID', strftime('%s','now') + 900);"
REMOTE

echo "15분 동안 한 번 쓸 수 있는 등록 주소($USER_ID):"
echo
echo "  $PUBLIC_URL/oauth/passkey-setup?code=$code"
echo
echo "아이폰(또는 패스키를 저장할 기기)의 브라우저에서 열고 '패스키 등록'을 누르세요."
