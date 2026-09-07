#!/usr/bin/env python3
"""career 세부 항목을 구조체 배열 → 단일 Markdown 필드로 전환 (2026-09).

`PortfolioCareerCompany.items: Vec<PortfolioCareerItem>`(title/period/bullets_markdown)를
`career_items_markdown: String` 하나로 바꾸면서, 이미 배포된 SQLite DB의 실제 데이터를
코드 init 로직이 아니라 이 스크립트로 직접 변환한다(`migrate_portfolio_markdown.py`와 같은 방침).

portfolio_section 테이블에서 section_key='career' 행의 content(JSON) 컬럼만 건드린다.
이 DB에는 다른 서비스 테이블도 같이 들어 있어서 스코프를 좁게 잡았다.

변환 규칙 (회사별):
    items: [{title, period?, bullets_markdown}, ...]
  →
    ## {title}
    _{period}_            (period가 있을 때만)

    {bullets_markdown}

  항목 사이는 빈 줄 하나. 변환 후 company에서 "items" 키는 삭제하고
  "career_items_markdown"를 넣는다. 이미 career_items_markdown가 있고 items가 없으면
  건드리지 않는다(멱등).

기본은 --dry-run(미리보기), 실제 반영은 --apply. 반드시 서비스 정지 + 백업 후 실행할 것
(README의 "프로덕션 마이그레이션 절차" 참고).

사용법:
    python3 scripts/migrate_career_items_markdown.py <db-path>            # 미리보기
    python3 scripts/migrate_career_items_markdown.py <db-path> --apply    # 반영
"""

import argparse
import json
import sqlite3
import sys


def item_to_markdown(item: dict) -> str:
    parts = [f"## {item.get('title', '').strip()}".rstrip()]
    period = (item.get("period") or "").strip()
    if period:
        parts.append(f"_{period}_")
    bullets = (item.get("bullets_markdown") or "").strip()
    if bullets:
        parts.append(bullets)
    return "\n\n".join(parts)


def migrate_career(content_json: str) -> tuple[str, bool]:
    career = json.loads(content_json)
    changed = False
    for company in career.get("companies", []):
        if "items" in company:
            items = company.pop("items") or []
            company["career_items_markdown"] = "\n\n".join(
                item_to_markdown(it) for it in items
            )
            changed = True
        elif "career_items_markdown" not in company:
            company["career_items_markdown"] = ""
            changed = True
    return json.dumps(career, ensure_ascii=False), changed


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
        "SELECT content FROM portfolio_section WHERE section_key = 'career'"
    ).fetchone()
    if row is None:
        print("[건너뜀] section_key=career 행이 없음")
        return 0

    new_content, changed = migrate_career(row["content"])
    if not changed:
        print("[변경없음] section_key=career")
        return 0

    print("[변경] section_key=career")
    print(f"  before: {row['content'][:400]}{'...' if len(row['content']) > 400 else ''}")
    print(f"  after : {new_content[:400]}{'...' if len(new_content) > 400 else ''}")

    if args.apply:
        conn.execute(
            "UPDATE portfolio_section SET content = ?, updated_at = CURRENT_TIMESTAMP "
            "WHERE section_key = 'career'",
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
