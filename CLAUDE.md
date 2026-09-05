# tyange-cms-api (CMS 백엔드)

Rust + Poem 프레임워크. cargo 사용.

## 기술 스택
- Rust
- Poem (HTTP 프레임워크) + poem-openapi (OpenAPI 스펙 자동 생성)
- SQLite + sqlx

## 연결 프로젝트
- 프론트엔드: `tyange-cms` (Nuxt 4 + Vue)
- API 변경 시 반드시 tyange-cms와 동기화 필요

## OpenAPI 스펙 (2026-09)

tyange-cms 쪽에서 실제 백엔드 없이 Prism 같은 도구로 mocking하려면 스펙이 필요해서 도입했다.
`poem`은 그대로 두고 `poem-openapi`만 얹었다 — Axum 마이그레이션 등 프레임워크 교체는 하지 않았다.

- `src/openapi.rs`: `#[OpenApi]`로 다시 짠 라우트들. 도메인별 struct(`PostsApi`, `TagsApi`,
  `PortfolioApi`, `ImagesApi`, `WeddingApi`)마다 `state: Arc<AppState>`를 필드로 들고 있다.
  `main.rs`에서 튜플로 묶어 `OpenApiService::new((...))`로 root("/")에 nest — 실제 서빙 경로가
  기존과 완전히 동일하게 유지된다(`/posts`, `/portfolio`, `/admin/posts` 등).
- **전부 옮기지 않았다.** tyange-cms가 실제로 호출하는 엔드포인트만 옮겼다:
  `/posts`(원래 있던 거 겸사겸사), `/admin/posts`, `/post/:id`, `/post/upload`,
  `/post/update/:id`, `/post/delete/:id`, `/tags-with-category`, `/portfolio`(GET/PUT/DELETE),
  `/portfolio/update`, `/portfolio/sections/:key`, `/upload-image`, `/admin/wedding/guests`.
  `/login`, `/login/google`, `/signup`, `/me`, `/me/profile`, `/admin/add-user`,
  `/public/wedding/guests`, push 관련 라우트는 plain poem으로 그대로 남아있다
  (tyange-cms가 안 쓰거나, RateLimit처럼 오퍼레이션 단위 미들웨어가 필요해서 OpenApi 변환과
  안 맞음). `/login`은 tyange-cms 자체에 dev bypass가 있어서 우선순위가 낮았다.
- `src/routes/*.rs`의 원래 핸들러는 **지운 게 아니라 등록만 뺐다** — `post_authorization_test`,
  `portfolio_routes_test`, `upload_image_test`, `wedding_guest_test`, `signup_test`가 여전히
  그 함수들을 직접 참조해서(TestClient로 단일 핸들러만 떼어 테스트) 지우면 테스트가 깨진다.
  실제로 살아있는 라우팅 테이블(`main.rs`)에는 안 걸리니 죽은 코드처럼 보이는 dead_code
  경고가 몇 개 뜨는데 의도된 것이다.
- 인증: `Auth`/`AdminOnly` 미들웨어 대신 `auth::authorization::AuthGuard`
  (`#[derive(SecurityScheme)]`, `Authorization` 헤더의 raw 토큰을 그대로 검증 — Bearer 아님)를
  씀. JWT 검증 로직 자체는 `middlewares::auth_middleware`와 동일한 걸 재사용. Admin 전용
  라우트는 `AuthGuard`로 인증만 받고 핸들러 안에서 기존 `ensure_admin()`을 그대로 호출해서
  401(미인증)과 403(권한 없음) 구분을 그대로 유지한다.
- `CustomResponse<T>`(기존 응답 envelope)에 `Object`를 직접 derive 못 했다 —
  `CustomResponse<()>`처럼 스키마로 표현 안 되는 인스턴스화가 이미 있어서(add_user 등),
  거기에 derive를 걸면 아직 안 옮긴 라우트가 전부 깨진다. 그래서 필드가 완전히 동일한
  `ApiResponse<T>`를 새로 만들어 OpenApi 라우트만 그쪽을 쓴다 — JSON 응답 포맷은 동일.
- `update_portfolio_section`의 `content` 필드는 `section_key`에 따라 다른 스키마로 런타임
  검증하는 진짜 다형적 필드라 OpenAPI로 정적 표현이 안 된다 —
  `poem_openapi::types::Any<serde_json::Value>`로 스펙상 "any"로 노출하고, 런타임 검증
  (`validate_portfolio_section`)은 그대로 뒀다.
- `ImagesApi::upload_image`에는 원래 `/upload-image`에 걸려있던 `SizeLimit` 미들웨어가 없다.
  poem의 `SizeLimit`은 `Content-Length` 헤더가 없는 요청을 전부 411로 거부해서 OpenApiService
  전체(GET 포함)에 걸 수 없고, OpenApiService는 오퍼레이션 단위 미들웨어도 지원 안 한다.
  게다가 `poem_openapi::types::multipart::Upload`는 크기 제한 없이 통째로 임시 파일에 먼저
  써버려서 핸들러 안에서 체크해도 늦다. 인증은 필수라 위험은 제한적이지만 완전한 하위 호환은
  아니다 — 옛 `/images/upload`(plain poem, SizeLimit 유지)는 그대로 뒀다.
- `services::image_storage::store_image`는 poem-openapi의 `Upload` 타입을 받도록 시그니처가
  바뀌었다. plain poem 쪽(`routes::upload_image`, 아직 테스트가 참조)은 `store_image_field`
  (`poem::web::Field` 버전)를 대신 쓴다 — 실제 저장 로직은 `store_image_from_reader`를 공유.
- 스펙은 서버를 띄운 채 `GET /api-docs/openapi.json`으로 받는다. Swagger UI는 `/api-docs`.
  별도 export 전용 바이너리는 안 만들었다 — DB 연결 없이 뽑고 싶어지면 그때 추가.

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
    워크플로가 배포마다 .env를 덮어쓰므로 `.forgejo/workflows/deploy.yml`의 .env 생성
    블록에 넣어야 유지된다
- CI를 GitHub Actions에서 Forgejo Actions로 전환 (2026-09) — `.github/workflows` 삭제,
  `.forgejo/workflows/deploy.yml`로 이전. 원래 워크플로가 `deploy on tyange-home-server`
  environment를 썼는데, 이건 조직 러너가 도는 EC2/lightsail 서버와는 다른(SSH로만 접속하는)
  별도 배포 대상이다. `runs-on: self-hosted`는 Docker 격리 없는 host 라벨(tyange-dev와
  동일 패턴)을 가리키는데, **이 러너 호스트에 Rust가 깔려 있는지 확인 안 됨** — 워크플로에
  `cargo` 없으면 rustup으로 설치하는 방어 스텝을 넣어뒀다. `uses:`는 전부
  `https://github.com/...` 완전한 URL 사용(data.forgejo.org 기본 레지스트리엔
  `appleboy/*` 서드파티 액션이 없음). Forgejo 쪽 secrets/vars(`HOST`/`USERNAME`/
  `SSH_PRIVATE_KEY`/`JWT_ACCESS_SECRET` 등, tyange-cms와 이름 체계가 다름 — 언더스코어
  없는 `USERNAME` 등 그대로 유지)는 자동으로 안 넘어와서 웹 UI에서 재등록 필요,
  Settings → Units → Overview의 Actions 체크박스도 켜야 함 — 둘 다 아직 확인 안 함
