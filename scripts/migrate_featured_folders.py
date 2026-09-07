#!/usr/bin/env python3
"""featured_projects: stack 필드 제거 + folder 필드 도입 (2026-09).

`PortfolioProject`에서 `stack: Vec<String>`을 없애고 `folder: String`을 넣었다.
tyange-dev가 slug를 하드코딩해 만들던 그룹("CMS 시스템", "개인 사이트")을 데이터로
옮기는 것이라, 기존 그룹핑을 그대로 재현하도록 folder 값을 시드한다.

portfolio_section 테이블에서 section_key='featured_projects' 행의 content(JSON 배열)만
건드린다. `migrate_portfolio_markdown.py` / `migrate_career_items_markdown.py`와 같은 방침.

변환 (프로젝트별):
  - "stack" 키가 있으면 삭제
  - "folder"가 없으면 slug로 아래 매핑을 적용, 매핑에 없으면 "" (단독 카드)
  이미 "stack"이 없고 "folder"가 있으면 건드리지 않는다(멱등).

기본은 --dry-run, 실제 반영은 --apply. 서비스 정지·백업 후 실행 권장.

사용법:
    python3 scripts/migrate_featured_folders.py <db-path>            # 미리보기
    python3 scripts/migrate_featured_folders.py <db-path> --apply    # 반영
"""

import argparse
import json
import sqlite3
import sys

# 기존 tyange-dev 하드코딩 그룹핑을 그대로 재현하는 시드 값.
SLUG_TO_FOLDER = {
    "tyange-cms": "CMS 시스템",
    "tyange-cms-api": "CMS 시스템",
    "tyange-blog": "개인 사이트",
    "tyange-dev": "개인 사이트",
    "tyange-dashboard": "개인 사이트",
}


def migrate_featured(content_json: str) -> tuple[str, bool]:
    projects = json.loads(content_json)
    changed = False
    for project in projects:
        if "stack" in project:
            del project["stack"]
            changed = True
        if "folder" not in project:
            project["folder"] = SLUG_TO_FOLDER.get(project.get("slug", ""), "")
            changed = True
    return json.dumps(projects, ensure_ascii=False), changed


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("db_path", help="포트폴리오 DB 파일 경로")
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--apply", action="store_true", help="실제로 DB에 반영한다")
    group.add_argument(
        "--dry-run", action="store_true", help="미리보기만(기본 동작, 명시용)"
    )
    args = parser.parse_args()

    conn = sqlite3.connect(args.db_path)
    conn.row_factory = sqlite3.Row

    integrity = conn.execute("PRAGMA integrity_check").fetchone()[0]
    if integrity != "ok":
        print(f"[중단] PRAGMA integrity_check 실패: {integrity}", file=sys.stderr)
        return 1

    row = conn.execute(
        "SELECT content FROM portfolio_section WHERE section_key = 'featured_projects'"
    ).fetchone()
    if row is None:
        print("[건너뜀] section_key=featured_projects 행이 없음")
        return 0

    new_content, changed = migrate_featured(row["content"])
    if not changed:
        print("[변경없음] section_key=featured_projects")
        return 0

    print("[변경] section_key=featured_projects")
    print(f"  before: {row['content'][:400]}{'...' if len(row['content']) > 400 else ''}")
    print(f"  after : {new_content[:400]}{'...' if len(new_content) > 400 else ''}")

    if args.apply:
        conn.execute(
            "UPDATE portfolio_section SET content = ?, updated_at = CURRENT_TIMESTAMP "
            "WHERE section_key = 'featured_projects'",
            (new_content,),
        )
        conn.commit()
        print("\n반영 완료(commit).")
    else:
        print("\n--dry-run 모드라 아무것도 반영 안 함. 반영하려면 --apply를 붙일 것.")

    conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
