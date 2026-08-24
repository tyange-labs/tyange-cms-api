# tyange-cms-api (CMS 백엔드)

Rust + Poem 프레임워크. cargo 사용.

## 기술 스택
- Rust
- Poem (HTTP 프레임워크)
- SQLite + sqlx

## 연결 프로젝트
- 프론트엔드: `tyange-cms` (Nuxt 3 + Vue)
- API 변경 시 반드시 tyange-cms와 동기화 필요

## 현재 상태
- 운영 중
- 포트폴리오 DB를 단일 JSON(content TEXT) → 섹션 테이블(portfolio_section)로 분리 (2026-04-13)
  - portfolio (마스터): portfolio_id, slug, created_at
  - portfolio_section: section_id, portfolio_id, section_key, content(JSON), created_at, updated_at
  - 사용 중인 섹션: meta, identity, featured_projects, career
  - 섹션별 개별 업데이트 엔드포인트: PUT /portfolio/sections/:section_key
  - 기존 API 응답 형태(PortfolioResponse)는 하위 호환 유지
- 이미지 업로드 공통 코어 분리 (2026-07-15)
  - services::image_storage::store_image: 매직 바이트 포맷 검증(png/jpg/gif/webp, SVG 제거), 크기 상한 스트리밍 읽기, 확장자는 판별된 포맷에서 도출
  - POST /images/upload (인증, 10MB)
  - images 테이블에 uploader_ip, status 컬럼 존재 (하위 호환용, 현재는 항상 uploader_ip=NULL, status='confirmed')
- 익명 이미지 업로드 엔드포인트 제거 (2026-08-04)
  - POST /public/images/upload, RateLimit 미들웨어, pending 이미지 자동 정리 태스크 모두 제거
- lovelog(모바일 청첩장) RSVP 연동 (2026-08-24)
  - CORS에 https://lovelog.tyange.com 추가 (lovelog는 순수 정적 SPA라 브라우저에서 직접 호출)
  - love.tyange.com / dev.tyange.com은 각각 SvelteKit adapter-static + prerender, Next.js output:"export"라 빌드 타임에만 호출 → CORS 불필요
  - POST /public/wedding/guests: 인증 없는 공개 쓰기. name(trim 후 1~20자), companions(0~10, 기본 0)
  - GET /admin/wedding/guests: AdminOnly + Auth. 하객 실명이 담기므로 반드시 인증 유지
  - wedding_guests 테이블: guest_id, name, companions, created_at
  - RateLimit 미들웨어 부활 (ce46308 구현 그대로): IP당 5회/분 + 30회/시간, 본문 4KB 상한
  - 리버스 프록시(tyange.com/api/cms) 뒤라 IP 식별에 TRUST_PROXY_FORWARDED_FOR=true 필요.
    워크플로가 배포마다 .env를 덮어쓰므로 workflow.yml의 .env 생성 블록에 넣어야 유지된다
