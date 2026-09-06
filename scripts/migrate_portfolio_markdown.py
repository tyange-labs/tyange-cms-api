#!/usr/bin/env python3
"""포트폴리오 markdown 필드 전환 마이그레이션 (2026-09).

tyange-cms-api의 PortfolioProject.highlights / PortfolioCareerItem.bullets를
Vec<String>에서 markdown 불릿 리스트 문자열(highlights_markdown / bullets_markdown)로
바꾸면서, 이미 배포된 SQLite DB의 실제 데이터를 코드 init 로직이 아니라 이 스크립트로
직접 변환한다(사용자 요청 — "DB 구조와 clean up은 백엔드에 init 코드를 심는 것으로
하지 말고 실제 DB를 정리해줬으면 함").

portfolio_section 테이블의 content(TEXT, JSON 문자열) 컬럼만 건드린다. 다른 테이블은
전혀 손대지 않는다 — 이 DB 파일에는 tyange-cms-api 외 다른 서비스 테이블도 같이 들어
있어서(budget_periods, user_matches, rss_feed_items 등) 각별히 스코프를 좁혔다.

기본은 --dry-run(아무것도 안 씀, 뭐가 바뀔지만 출력)이고, 실제로 반영하려면
--apply를 명시해야 한다. 반드시 서비스를 멈추고, 백업을 먼저 뜨고 실행할 것
(README의 "프로덕션 마이그레이션 절차" 참고 — 이 스크립트 자체는 그 절차의
한 단계일 뿐, 백업/서비스 정지는 스크립트 밖에서 사람이 확인하고 진행한다).

사용법:
    python3 scripts/migrate_portfolio_markdown.py <db-path> --dry-run   # 기본값, 미리보기만
    python3 scripts/migrate_portfolio_markdown.py <db-path> --apply     # 실제 반영
"""

import argparse
import json
import sqlite3
import sys


def highlights_to_markdown(highlights: list[str]) -> str:
    return "\n".join(f"- {item}" for item in highlights)


def migrate_featured_projects(content_json: str) -> tuple[str, bool]:
    projects = json.loads(content_json)
    changed = False
    for project in projects:
        if "highlights" in project:
            project["highlights_markdown"] = highlights_to_markdown(project.pop("highlights"))
            changed = True
        elif "highlights_markdown" not in project:
            # 이미 변환됐거나 애초에 없던 경우 — 스키마가 요구하는 기본값(빈 문자열)만 채움
            project["highlights_markdown"] = ""
            changed = True
    return json.dumps(projects, ensure_ascii=False), changed


def migrate_career(content_json: str) -> tuple[str, bool]:
    career = json.loads(content_json)
    changed = False
    for company in career.get("companies", []):
        for item in company.get("items", []):
            if "bullets" in item:
                item["bullets_markdown"] = highlights_to_markdown(item.pop("bullets"))
                changed = True
            elif "bullets_markdown" not in item:
                item["bullets_markdown"] = ""
                changed = True
    return json.dumps(career, ensure_ascii=False), changed


# PortfolioIdentity에 실제로 존재하는 필드만 남긴다 — validate_portfolio_section이
# 예전엔 원본 JSON을 그대로 재저장해서 role/location/availability 같은 레거시 키가
# 계속 살아남았다(이번에 그 버그는 코드에서 고쳤지만, 이미 저장된 데이터는 별개로
# 정리해야 함).
IDENTITY_KNOWN_FIELDS = {"name", "email", "github_url"}


def migrate_identity(content_json: str) -> tuple[str, bool]:
    identity = json.loads(content_json)
    stale_keys = [k for k in identity if k not in IDENTITY_KNOWN_FIELDS]
    if not stale_keys:
        return content_json, False
    for key in stale_keys:
        del identity[key]
    return json.dumps(identity, ensure_ascii=False), True


MIGRATORS = {
    "featured_projects": migrate_featured_projects,
    "career": migrate_career,
    "identity": migrate_identity,
}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("db_path", help="포트폴리오 DB 파일 경로")
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--apply", action="store_true", help="실제로 DB에 반영한다")
    group.add_argument(
        "--dry-run", action="store_true", help="미리보기만 하고 아무것도 안 씀(기본 동작과 동일, 명시용)"
    )
    args = parser.parse_args()

    conn = sqlite3.connect(args.db_path)
    conn.row_factory = sqlite3.Row

    integrity = conn.execute("PRAGMA integrity_check").fetchone()[0]
    if integrity != "ok":
        print(f"[중단] PRAGMA integrity_check 실패: {integrity}", file=sys.stderr)
        return 1

    portfolio_documents_exists = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='portfolio_documents'"
    ).fetchone()
    if portfolio_documents_exists:
        count = conn.execute("SELECT COUNT(*) FROM portfolio_documents").fetchone()[0]
        print(f"[안내] 레거시 portfolio_documents 테이블이 존재함 (행 {count}개) — 이 스크립트는 안 건드림, 별도 확인 필요")

    any_change = False
    for section_key, migrator in MIGRATORS.items():
        row = conn.execute(
            "SELECT content FROM portfolio_section WHERE section_key = ?", (section_key,)
        ).fetchone()
        if row is None:
            print(f"[건너뜀] section_key={section_key} 행이 없음")
            continue

        new_content, changed = migrator(row["content"])
        if not changed:
            print(f"[변경없음] section_key={section_key}")
            continue

        any_change = True
        print(f"[변경] section_key={section_key}")
        print(f"  before: {row['content'][:200]}{'...' if len(row['content']) > 200 else ''}")
        print(f"  after : {new_content[:200]}{'...' if len(new_content) > 200 else ''}")

        if args.apply:
            conn.execute(
                "UPDATE portfolio_section SET content = ?, updated_at = CURRENT_TIMESTAMP "
                "WHERE section_key = ?",
                (new_content, section_key),
            )

    if args.apply and any_change:
        conn.commit()
        print("\n반영 완료(commit).")
    elif not args.apply:
        print("\n--dry-run 모드라 아무것도 반영 안 함. 실제 반영하려면 --apply를 붙여서 재실행할 것.")
    else:
        print("\n변경 사항 없음.")

    conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
