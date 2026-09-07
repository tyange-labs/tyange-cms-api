#!/usr/bin/env bash
#
# featured_projects: summary + highlights_markdown → body_markdown 하나로 병합 (2026-09).
#
# PortfolioProject에서 summary(문단) + highlights_markdown(불릿 리스트) 두 필드를 없애고
# body_markdown 하나로 합쳤다(career_items_markdown과 같은 방향). 병합 규칙:
#   body_markdown = trim(summary) + "\n\n" + trim(highlights_markdown)
#   (한쪽이 비어 있으면 다른 쪽만)
# 그리고 summary / highlights_markdown 키는 삭제한다.
#
# portfolio_section 테이블에서 section_key='featured_projects' 행의 content(JSON 배열)만
# 건드린다. 이미 body_markdown이 있고 두 옛 필드가 없으면 건드리지 않는다(멱등).
#
# 의존: sqlite3, jq. 기본은 미리보기, 실제 반영은 --apply.
#
# 사용법:
#   scripts/migrate_featured_body.sh <db-path>            # 미리보기
#   scripts/migrate_featured_body.sh <db-path> --apply    # 반영
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

after="$(jq -c '
  def trim: (. // "") | sub("^\\s+"; "") | sub("\\s+$"; "");
  map(
    if has("body_markdown") then .
    else
      (.summary | trim) as $s
      | (.highlights_markdown | trim) as $h
      | .body_markdown = (
          if   $s == "" then $h
          elif $h == "" then $s
          else $s + "\n\n" + $h
          end
        )
    end
    | del(.summary, .highlights_markdown)
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
  # readfile()은 BLOB을 돌려주므로 CAST(... AS TEXT) 필수 (sqlx가 TEXT 기대).
  sqlite3 "$DB" "UPDATE portfolio_section
                 SET content = CAST(readfile('$tmp') AS TEXT), updated_at = CURRENT_TIMESTAMP
                 WHERE section_key = 'featured_projects';"
  echo
  echo "반영 완료(commit)."
else
  echo
  echo "미리보기 모드. 반영하려면 --apply를 붙일 것."
fi
