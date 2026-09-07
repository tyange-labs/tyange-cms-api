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
  `/login`, `/signup`, `/me`, `/me/profile`, `/admin/add-user`, `/public/wedding/guests`는
  plain poem으로 그대로 남아있다(tyange-cms가 안 쓰거나, RateLimit처럼 오퍼레이션 단위
  미들웨어가 필요해서 OpenApi 변환과 안 맞음). `/login`은 tyange-cms 자체에 dev bypass가
  있어서 우선순위가 낮았다.
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
  별도 배포 대상이다. `runs-on: self-hosted`는 (한때 Docker 격리 없는 host 라벨로
  알고 있었으나, 2026-09-06에 `docker ps`/`docker top`으로 직접 확인해보니 실제로는
  **매 실행마다 새로 뜨는 `node:lts-bookworm` 컨테이너**였다 — 조직 러너가
  `self-hosted:docker://node:lts-bookworm` 하나로만 등록돼 있고 로그엔 라벨 이름만
  찍혀서 헷갈렸음) — 즉 Rust도 cargo 캐시도 컨테이너와 함께 매번 사라진다. 워크플로에
  `cargo` 없으면 rustup으로 설치하는 방어 스텝을 넣어뒀다. `uses:`는 전부 `https://github.com/...` 완전한
  URL 사용(data.forgejo.org 기본 레지스트리엔 `appleboy/*`/`actions/cache` 같은
  서드파티 액션이 없음). Forgejo 쪽 secrets/vars(`HOST`/`USERNAME`/
  `LIGHTSAIL_SSH_KEY`/`JWT_ACCESS_SECRET`/`DATABASE_PATH`/`UPLOAD_PATH`/
  `SERVICE_PATH` 등)는 자동으로 안 넘어와서 웹 UI에서 재등록 필요 — 2026-09-06에
  사용자가 등록함(tyange-cms의 `DEPLOY_PATH` 등 vars 일부는 처음에 빠뜨려서 배포가
  SCP 단계에서 실패했었고, 그 뒤 채워 넣어 해결). Settings → Units → Overview의
  Actions 체크박스도 켜야 함.
- CI 빌드 캐시를 러너 볼륨 마운트로 (2026-09) — `actions/cache`는 이 Forgejo 러너
  (forgejo-runner v13.1.0, config 없이 기본값)에서 컨테이너에 `ACTIONS_CACHE_URL`이
  안 주입돼 **한 번도 저장된 적이 없었다**(멀티홈 박스라 cache-host 자동탐지 실패로 추정).
  매 빌드가 콜드였고 run #7이 2h3m. 대신 **Lightsail 박스**(러너·Forgejo·배포 대상이
  전부 여기 = `100.96.219.103`, tailnet `cms`)에:
  - `/home/ubuntu/forgejo-runner/config.yaml` — `container.options`로
    `/home/ubuntu/ci-cache/{dot-cargo,dot-rustup,cms-api-target}`를 각각
    `/root/.cargo`, `/root/.rustup`, `/ci-target`에 bind mount. `valid_volumes`로
    그 경로 허용. `runner.capacity=1`(기본)이라 잡 직렬 → 공유 안전.
  - systemd drop-in `/etc/systemd/system/forgejo-runner.service.d/10-config.conf` —
    `ExecStart`에 `--config` 추가(기본 unit은 CLI 플래그만 씀).
  - 워크플로: `CARGO_TARGET_DIR=/ci-target`, `cargo test`에 `--release`(Build의
    release 산출물 재사용, 안 그러면 디버그로 전체 재컴파일), deploy는
    `$CARGO_TARGET_DIR/release/`에서 복사. `actions/cache` 스텝 제거.
  - 박스에 swap 2GB 추가(`/swapfile`, `vm.swappiness=10`) — RAM 1.9GB뿐이라 콜드
    빌드 중 OOM 위험이 있었다.
  첫 빌드만 콜드, 이후 웜. config.yaml·drop-in·`ci-cache/`는 버전관리 밖(박스에만 있음).
- Google 로그인 / Web Push 기능 완전 제거 (2026-09) — 실사용 안 해서 걷어냄.
  `auth/google.rs`, `routes/login_google.rs`, `web_push.rs`, push 라우트 4개 삭제.
  `users.google_sub` 컬럼과 `web_push_subscriptions` 테이블은 `init_db()`에서
  `DROP COLUMN`/`DROP TABLE`로 정리(반복 실행해도 안전). `users.auth_provider`는 범용
  컬럼이라 유지(항상 'local'). `Cargo.toml`에서 `web-push` 의존성 제거(`reqwest`는
  blog_redeploy가 계속 써서 유지).
- 블로그 자동 재배포를 GitHub `repository_dispatch`에서 Forgejo `workflow_dispatch` API로
  이식 (2026-09) — Forgejo(v16.0.3)엔 GitHub의 repository_dispatch(커스텀 이벤트 + 임의
  워크플로 리스닝)에 대응하는 기능이 없다. 대신
  `POST /repos/{owner}/{repo}/actions/workflows/{file}/dispatches`
  (body: `{ref, inputs: {content_event, post_id, visibility}}`,
  인증 헤더는 `Bearer`가 아니라 `Authorization: token <TOKEN>`)를 쓴다. `tyange-blog`의
  기존 워크플로에 이미 `on: workflow_dispatch` + 동일한 inputs 스키마가 있어서 그쪽은
  트리거 정의(`repository_dispatch` 블록)만 빼면 됐다. 성공 판정도 정확히 204만 보던 것에서
  `is_success()`(2xx 전체)로 일반화 — `return_run_info`에 따라 204/201이 갈릴 수 있어서.
  `TYANGE_BLOG_REDEPLOY_TOKEN`은 이제 GitHub PAT가 아니라 해당 저장소의 Actions 실행 권한을
  가진 Forgejo API 토큰이어야 한다.
- 포트폴리오 죽은 라우트/테스트 정리 + markdown 필드 도입 (2026-09) — tyange-cms의
  포트폴리오 편집 UX가 너무 힘들다는 문제(중첩 폼, `highlights`/`bullets` 배열을
  textarea에서 `join('\n')`/`split('\n')`으로 왕복시키다 마지막 줄 Enter가 씹히는
  버그)를 markdown 기반 편집으로 풀었다.
  - 선행 정리: `src/routes/{get,update,update_section,delete}_portfolio.rs`(등록만
    안 된 죽은 사본, `openapi.rs::PortfolioApi`가 실제 서빙)와 그걸 테스트하던
    `portfolio_routes_test.rs`(즉 cargo test가 통과해도 실서비스 라우트는 검증 안
    됐음)를 정리 — 테스트를 실제 `PortfolioApi`(OpenApiService를 TestClient에 직접
    물림) 대상으로 재작성.
  - `PortfolioProject.highlights: Vec<String>` → `highlights_markdown: String`,
    `PortfolioCareerItem.bullets: Vec<String>` → `bullets_markdown: String`으로 변경.
    배열 파싱 왕복 자체가 없어져서 그 버그가 구조적으로 해결됨. `summary`/
    `intro.content`는 타입 변경 없이(이미 `String`) "markdown일 수 있음"으로 의미
    전환만 — 백엔드는 여전히 markdown을 파싱하지 않고 문자열 그대로 저장/반환한다
    (렌더링은 프론트엔드 담당, 새 markdown 관련 의존성 추가 없음).
  - `validate_portfolio_section`이 원본 JSON이 아니라 **파싱된 구조체를 재직렬화**
    하도록 수정 — 예전엔 스키마에 없는 키(`identity.role`/`location`/`availability`
    등 레거시)가 원본 값 그대로 재저장돼서 운영 DB에 죽은 필드가 계속 쌓였다.
  - `update_portfolio`/`delete_portfolio`의 여러 SQL을 `pool.begin()`/`commit()`
    트랜잭션으로 묶음 — 예전엔 각 statement가 독립 커밋돼서 중간 실패 시 반쪽짜리
    문서가 남을 수 있었다. `upsert_portfolio_section` 헬퍼도 `sqlx::Executor`
    제네릭으로 바꿔서 pool/트랜잭션 양쪽에서 재사용 가능하게 함.
  - 운영 DB의 실제 데이터(레거시 `identity` 키, `highlights`/`bullets` 배열 모양,
    남아있을 수 있는 `portfolio_documents` 테이블)는 코드 마이그레이션이 아니라
    `scripts/migrate_portfolio_markdown.py`를 SSH로 직접 실행해 변환했다 — 자세한
    내용과 백업 절차는 그 스크립트 커밋 메시지 참고.
- career 세부 항목을 구조체 배열 → 단일 Markdown으로 (2026-09) — 편집 UX(회사 안에
  항목마다 title/period/bullets를 또 Accordion으로 접는 3중 중첩)가 싫다는 요청.
  `PortfolioCareerItem` 구조체 제거, `PortfolioCareerCompany.items: Vec<PortfolioCareerItem>`
  → `career_items_markdown: String`. 회사 스칼라 필드(company/period/employment_type/
  role/position)는 그대로. 운영 DB는 `scripts/migrate_career_items_markdown.py`로 변환
  (항목별 `## {title}` + `_{period}_` + bullets를 빈 줄로 이어붙임, 멱등). tyange-cms
  에디터는 회사별 마크다운 필드 하나로, tyange-dev는 `company.career_items_markdown`를
  한 번 렌더하도록 동기화.
