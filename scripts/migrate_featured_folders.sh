#!/usr/bin/env bash
#
# featured_projects: stack 필드 제거 + folder 필드 도입 (2026-09).
#
# PortfolioProject에서 stack: Vec<String>을 없애고 folder: String을 넣었다.
# tyange-dev가 slug를 하드코딩해 만들던 그룹("CMS 시스템", "개인 사이트")을
# 데이터로 옮기는 것이라 기존 그룹핑을 그대로 재현하도록 folder 값을 시드한다.
#
# portfolio_section 테이블에서 section_key='featured_projects' 행의 content(JSON 배열)만
# 건드린다. 이 DB에는 다른 서비스 테이블도 같이 들어 있어서 스코프를 좁게 잡았다.
#
# 변환 (프로젝트별):
#   - "stack" 키가 있으면 삭제
#   - "folder"가 없으면 slug로 아래 매핑을 적용, 매핑에 없으면 "" (단독 카드)
#   이미 stack이 없고 folder가 있으면 건드리지 않는다(멱등).
#
# 의존: sqlite3, jq. 기본은 미리보기, 실제 반영은 --apply. 서비스 정지·백업 후 실행 권장.
#
# 사용법:
#   scripts/migrate_featured_folders.sh <db-path>            # 미리보기
#   scripts/migrate_featured_folders.sh <db-path> --apply    # 반영
#
set -euo pipefail

DB="${1:-}"
MODE="${2:-}"

if [[ -z "$DB" ]]; then
  echo "사용법: $0 <db-path> [--apply]" >&2
  exit 2
fi
command -v sqlite3 >/dev/null || { echo "[중단] sqlite3 필요" >&2; exit 1; }
command -v jq >/dev/null || { echo "[중단] jq 필요" >&2; exit 1; }
[[ -f "$DB" ]] || { echo "[중단] DB 파일 없음: $DB" >&2; exit 1; }

integrity="$(sqlite3 "$DB" 'PRAGMA integrity_check;')"
[[ "$integrity" == "ok" ]] || { echo "[중단] PRAGMA integrity_check 실패: $integrity" >&2; exit 1; }

before="$(sqlite3 "$DB" "SELECT content FROM portfolio_section WHERE section_key='featured_projects';")"
if [[ -z "$before" ]]; then
  echo "[건너뜀] section_key=featured_projects 행이 없음"
  exit 0
fi

# 기존 tyange-dev 하드코딩 그룹핑을 그대로 재현하는 slug→folder 시드.
after="$(jq -c '
  def folder_for($slug):
    {
      "tyange-cms":     "CMS 시스템",
      "tyange-cms-api": "CMS 시스템",
      "tyange-blog":    "개인 사이트",
      "tyange-dev":     "개인 사이트",
      "tyange-dashboard": "개인 사이트"
    }[$slug] // "";
  map(
    del(.stack)
    | if has("folder") then . else .folder = folder_for(.slug) end
  )
' <<<"$before")"

if [[ "$before" == "$after" ]]; then
  echo "[변경없음] section_key=featured_projects"
  exit 0
fi

echo "[변경] section_key=featured_projects"
echo "  before: ${before:0:400}"
echo "  after : ${after:0:400}"

if [[ "$MODE" == "--apply" ]]; then
  tmp="$(mktemp)"
  trap 'rm -f "$tmp"' EXIT
  printf '%s' "$after" > "$tmp"
  # readfile()로 JSON을 그대로 넣는다 — SQL 문자열 escape 이슈 없음.
  # readfile()은 BLOB을 돌려주므로 CAST(... AS TEXT) 필수 — 안 그러면 content 컬럼이
  # BLOB으로 저장돼 sqlx(TEXT 기대)가 디코딩에 실패한다(실측: 500).
  sqlite3 "$DB" "UPDATE portfolio_section
                 SET content = CAST(readfile('$tmp') AS TEXT), updated_at = CURRENT_TIMESTAMP
                 WHERE section_key = 'featured_projects';"
  echo
  echo "반영 완료(commit)."
else
  echo
  echo "미리보기 모드. 반영하려면 --apply를 붙일 것."
fi
