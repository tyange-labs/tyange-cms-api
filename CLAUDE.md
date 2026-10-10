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
  `/login`, `/me`, `/me/profile`, `/admin/add-user`, `/public/wedding/guests`는
  plain poem으로 그대로 남아있다(tyange-cms가 안 쓰거나, RateLimit처럼 오퍼레이션 단위
  미들웨어가 필요해서 OpenApi 변환과 안 맞음). `/login`은 tyange-cms 자체에 dev bypass가
  있어서 우선순위가 낮았다.
- `src/routes/*.rs`의 원래 핸들러는 **지운 게 아니라 등록만 뺐다** — `post_authorization_test`,
  `portfolio_routes_test`, `upload_image_test`, `wedding_guest_test`, `account_test`가 여전히
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
- **gotolib 찜 소장 알림 (Web Push)** (2026-10, `feat/gotolib-push`): 찜 목록의 책이 '자주 가는 도서관'에 **미소장 → 소장**으로
  바뀌면 알린다. 모듈 `src/gotolib_push.rs`(+`gotolib_push/tests.rs`), 라우트 `routes/gotolib_push.rs`. 크레이트는
  `web-push-native`(요청만 만들고 전송은 reqwest; OpenSSL 불필요 — 옛 `web-push`+isahc는 쓰지 않는다).
  - **대상은 패스키(동기화) 계정만**: 서버는 동기화 문서(`gotolib_sync.data`)의 `watchlist`(isbn13·title)와 `favLibraries`(libCode·libName)를
    읽는다. 앱의 문서 모양이 바뀌면 `parse_targets`를 같이 고칠 것.
  - API: `GET /gotolib/push/key`(공개, VAPID 공개 키), `POST /gotolib/push/subscribe`·`unsubscribe`·`test`(로그인). 꺼져 있으면 503.
    **구독 endpoint는 알려진 푸시 서비스 호스트(fcm.googleapis.com, push.apple.com, push.services.mozilla.com, notify.windows.com)만 받는다** —
    서버가 그 주소로 POST를 보내므로 SSRF 방지다. 계정당 구독 5개(넘으면 오래된 것부터 삭제), 410/404나 5번 연속 실패한 구독은 삭제.
  - 수집 작업 한 개가 12시간마다(`GOTOLIB_PUSH_INTERVAL_HOURS`; 마지막 실행 시각은 DB `gotolib_push_runs`에 있어 재시작해도 주기를 지킨다)
    (책, 도서관)마다 `bookExist`를 **우리 nginx 프록시(`GOTOLIB_LIBRARY_BASE`, 기본 https://gotolib.tyange.com/api/library/)로** 부른다 — 키가
    nginx에만 있어서다. 라운드당 상한 300회(`GOTOLIB_PUSH_MAX_CHECKS`), 같은 쌍은 계정이 달라도 한 번. **도서관 정보나루의 일일 호출 한도는
    확인하지 못했다**(앱의 직접 조회와 같은 키를 쓴다) — 한도 오류가 보이면 주기를 늘릴 것.
  - 알림 조건: 지난번 상태가 **미소장**이었던 쌍이 소장이 됐을 때만. 처음 보는 쌍은 상태만 기록(이미 소장인 책을 쏟아내지 않는다). 모든 기기에
    전달이 실패하면 상태를 바꾸지 않아 다음 라운드에 다시 시도. 4권 이상이 한꺼번에 소장되면 한 통으로 합친다.
  - VAPID 키는 서버가 처음 켜질 때 만들어 `VAPID_KEY_PATH`(기본 `./data/vapid_private.key`, 권한 600)에 보관한다 — 비밀값을 Forgejo에 두지 않으려고.
    **이 파일을 잃으면 기존 구독이 못 쓰게 되어 앱이 다시 구독해야 한다**(백업 대상). 표: `gotolib_push_subscriptions`·`gotolib_watch_state`·`gotolib_push_runs`
    (이 DB를 쓰는 다른 앱의 `web_push_*`와 겹치지 않게 gotolib_ 접두). 비활성 계정 정리에 두 표도 포함.
  - **기본 켜짐**: 배포 워크플로가 `GOTOLIB_PUSH_ENABLED`에 vars가 없으면 `true`를 넣는다. 끄려면 vars `GOTOLIB_PUSH_ENABLED=false` + 재배포.
  - 테스트: 가짜 푸시 서비스·가짜 도서관 API(로컬 poem 서버)로 암호화·VAPID 서명·복호화(`web_push_native::decrypt`)까지 검증한다. 변이 검사 8종
    (처음 보는 쌍 알림, 호스트 허용 목록, 전달 실패 시 상태 갱신, 호출 상한, 중복 제거, 410 삭제, 상태 정리, 남의 구독 삭제)로 확인함.
- **gotolib 책 표지** (2026-10, `feat/gotolib-covers`): ISBN으로 알라딘 상품 페이지(`/shop/wproduct.aspx`)의
  `og:image`를 찾아 **200px 안팎의 JPEG 썸네일로 줄여 `COVERS_DIR`(기본 `./data/covers`)에 저장**한다. 모듈
  `src/covers.rs`(+`covers/tests.rs`), 라우트 `routes/gotolib_covers.rs`. 원본은 저장하지 않는다.
  - `GET /gotolib/covers?isbn=A,B,C`(로그인 없음, 최대 50개, IP 분당 60·시간당 600): DB만 보고 `ok`(+`url`)/`pending`/
    `missing`을 돌려준다. 모르는 ISBN은 수집 대기로 등록하되 큐(`COVER_QUEUE_MAX`, 기본 500)가 차면 등록하지 않는다.
    잘못된 ISBN이 하나라도 있으면 400. **알라딘 호출은 하지 않는다.** 이미지는 `/gotolib/cover-images/<isbn>.jpg`
    (정적 제공, 30일 캐시, 폴더 목록 없음).
  - **알라딘 호출은 수집 작업 한 개(`covers::run_worker`)만 한다.** 3초 이상 간격(환경 변수로 늘릴 수만 있음), 동시 1건,
    `User-Agent`는 `gotolib-covers/0.1 (+연락처)`(연락처는 `COVER_CONTACT`, 없으면 https://gotolib.tyange.com). **403/429면 작업 전체가 6시간 쉰다**(행 하나의 실패가 아니라
    막힘으로 본다). 표지 없음은 30일 뒤 재확인, 일시 오류는 10분부터 두 배씩(최대 12시간) 5번 뒤 `missing`.
    og:image는 `https://image.aladin.co.kr/product/…`만 믿는다(다른 호스트는 받지 않는다). `/search/`·`/ttb/`·
    `/account/` 등 알라딘 robots.txt가 막는 경로는 부르지 않는다. 약관에 자동 수집 금지 조항이 없음을 사용자가 확인함.
  - **코드는 `COVER_FETCH_ENABLED=true`일 때만 돈다**(`COVER_CONTACT`는 선택). 배포 워크플로는 vars가 없으면 `true`를 넣는다(기본 켜짐). 배포는 `.forgejo/workflows/deploy.yml`이
    Forgejo vars(`COVER_FETCH_ENABLED`, 선택으로 `COVER_CONTACT`·`COVERS_DIR`)로 `.env`에 넣는다 — 끄려면 vars `COVER_FETCH_ENABLED=false`를 만들고 다시 배포한다.
    이미 저장된 썸네일은 계속 제공된다. 서버가 1.9GB라 작업은 동시 1건, 이미지 변환은 `spawn_blocking`.
  - 테스트: 가짜 알라딘(로컬 poem 서버, `SourceConfig.allow_http`)으로 정상/표지 없음/403/500/이미지 아님/외부 호스트를
    검증한다. **실제 알라딘을 부르는 테스트는 만들지 않는다.** 변이 검사(검사 숫자·호스트 검증·막힘 처리·큐 상한·확대 방지)로 확인함.
  - 주의: 이 워크플로는 `pull_request`에도 걸려 있어 PR을 열면 빌드 후 **배포 단계까지** 돈다(기존 동작). 이 브랜치로 PR을 열지 말 것.
- **패스키(WebAuthn) 로그인 + gotolib 동기화 추가** (2026-10, `feat/passkeys`). `webauthn-rs`(0.5,
  `conditional-ui` feature = discoverable 로그인용). 로그인 검증이 끝나면 기존 `issue_login_response()`로
  같은 JWT를 발급하므로 `Auth`/`AuthGuard`는 바뀐 게 없다. 모듈: `src/passkey.rs`(설정·진행 중인 의식
  상태, `blog_redeploy`처럼 lib+bin 양쪽에 `mod` 선언 필요), `routes/passkeys.rs`, `routes/gotolib_sync.rs`.
  - 의식 상태(`PasskeyRegistration`/`DiscoverableAuthentication`)는 **메모리**에 5분 보관하고 한 번 꺼내면
    폐기한다. 단일 프로세스라 DB 직렬화(`danger-allow-state-serialisation`)를 쓰지 않았다 — 서버를
    여러 대로 늘리면 `login/start`와 `finish`가 다른 인스턴스로 갈 수 있어 깨진다.
  - 등록 옵션에 `residentKey: preferred`를 덧붙인다(webauthn-rs의 passkey 등록은 레지던트 키를 요구하지
    않는다). 동기화 패스키(iCloud·Google·1Password)는 알아서 discoverable이지만 보안 키는 요청해야 만든다.
    그래도 discoverable이 아닌 자격 증명은 사용자명 없는 로그인이 안 되므로 비밀번호 로그인은 복구용으로 남긴다.
  - 테스트는 `webauthn-authenticator-rs`의 `SoftPasskey`(dev-dependency)로 실제 서명 검증을 거친다. 이
    인증기는 레지던트 키가 없고 응답에 user handle을 안 채워서, 테스트가 allowCredentials와 user handle을
    직접 채운다(`passkeys_test.rs` 주석). origin 불일치 거부는 변이 검사로 확인했다.
  - `PASSKEY_RP_ID` 미설정이면 전부 503(기능 꺼짐). 운영 환경변수는 README "패스키 환경변수".
- **gotolib 전용 계정(가입)** (2026-10-02): `POST /passkeys/signup/{start,finish}`로 이메일·비밀번호 없이
  패스키만으로 계정을 만든다(`users.user_role = 'gotolib'`, `password NULL`, `user_id = gl-<uuid>`).
  - **블로그·CMS와 `users` 테이블을 공유**하므로, 글·이미지를 쓰는 엔드포인트(`/post/upload`·`update`·
    `delete`, `/upload-image`, `/images/upload`)에 `ensure_content_writer`(admin·user 허용 목록)를 걸었다.
    새 역할이 생겨도 기본값이 '쓸 수 있음'이 되지 않게 **허용 목록**으로 막는다. 이전에는 로그인만 되면
    누구나 글을 올릴 수 있었고, 공개 `/signup`이 열려 있던 동안 그 구멍이 실제로 열려 있었다.
    새 라우트에 `AuthGuard`만 걸고 역할 확인을 빼먹지 않도록 주의.
  - 상한: 계정 총량(`GOTOLIB_MAX_ACCOUNTS`), 가입 IP 속도 제한, 동기화 문서 512KB, 365일 비활성 계정 정리
    (`db::purge_inactive_gotolib_accounts`, 시작 시 + 24시간마다). 서버가 저장하는 건 사용자당 JSON 하나.
  - 가입 챌린지(`Pending::Signup`)와 기존 '패스키 추가'(`Pending::Register`)는 서로 바꿔 쓸 수 없다(테스트로 고정).
  - `GET /gotolib/sync`는 `ETag`/304를 지원한다(클라이언트의 전송량을 줄이려고).
  - **패스키 표시 이름**: gotolib 계정의 패스키는 WebAuthn `user.name`/`displayName`을 `gotolib`으로 보낸다
    (`GOTOLIB_PASSKEY_NAME`). 패스키 선택 창(iOS·macOS·Chrome)이 이 값을 그대로 보여주는데, 예전엔 계정 id
    `gl-8d9fd7414a28…`가 나와서 알아볼 수 없었다. 계정은 `user.id`(사용자 핸들)로 구분하므로 이름이 같아도
    로그인에는 영향이 없다. **이미 만든 패스키의 이름은 바뀌지 않는다**(기기에 저장된 값) — 같은 계정에 새 패스키를
    만들면(같은 RP·사용자 핸들) iCloud 키체인은 이전 것을 덮어쓰고 이름도 새 값이 된다. 블로그·CMS 계정(user·admin)은
    예전처럼 아이디를 이름으로 쓴다.
- 공개 `POST /signup` 제거 (2026-10). 동기화 저장소가 생기므로 아무나 계정을 만들 수 없게 닫았다.
- `portfolio_pdf` 포트폴리오 섹션 추가 (2026-10) — tyange-cms `/portfolio-pdf`(PDF 포트폴리오 편집기)가 쓰는 문서.
  `PUT /portfolio/sections/portfolio_pdf`로 저장하고, 새로 만든 **`GET /portfolio/sections/:section_key`**(관리자
  전용, `AuthGuard` + `ensure_admin`)로 읽는다. 공개 `GET /portfolio` 조립부는 알 수 없는 섹션을 무시해서
  `portfolio_pdf`은 공개 응답에 섞이지 않는다(테스트로 고정). 검증은 `{ theme, pages: [{ id, template, dark, data }] }`
  모양과 페이지 40장·직렬화 256KB 상한·id 비어 있음/중복만 본다 — `data`는 템플릿마다 달라 구조를 안
  본다(렌더링은 프론트엔드 담당). 테스트 3개(`portfolio_routes_test.rs`) 추가로 48개.
- `/admin/posts` 목록에만 `excerpt`(본문 앞 300자, 마크다운 원문) 추가 (2026-09). tyange-cms 목록에서
  제목 없는 짧은 글을 구분하려고 쓴다. `PostItem.excerpt`는 `Option`이고 비어 있으면 JSON에서 빠지므로
  `/posts`·`/posts/search-with-tags` 응답 모양은 그대로다.
- 포스트 응답(`PostItem`/`Post` — `/posts`, `/admin/posts`, `/posts/search-with-tags`, `/post/:id`,
  수정 응답)에 `created_at` 추가 (2026-09). lovelog가 하루에 여러 개 올라가는 짧은 글에 시각을
  찍으려고 쓴다(`published_at`은 날짜뿐이고 블로그·dev·love가 그 모양에 기대고 있어 건드리지 않음).
  SQLite `CURRENT_TIMESTAMP`(UTC)를 `strftime`으로 `2026-09-24T10:18:26Z` 모양으로 내보내고,
  파싱 안 되는 옛 값은 `''`로 떨어뜨린다 — 한 행 때문에 목록 전체가 디코드 실패하지 않게.
  글을 처음 저장한 시각이라 수정할 수 없다.
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
  - POST /public/wedding/guests: 인증 없는 공개 쓰기. name(trim 후 1~20자), companions(0~10, 기본 0),
    visit_hour(필수, 12~16 — 전시가 12시에 열고 5시에 닫아 한 시간짜리 시간대의 시작 시각. 생략하면 400)
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
- **CI에서 테스트 단계 제거 (2026-10)** — push·PR 배포는 `cargo build --release`만 하고 `cargo test`는 돌리지 않는다.
  테스트 컴파일이 20분 안팎(전체 배포 35~50분 중 가장 큼)이라서다. **푸시 전에 로컬에서 `cargo test`를 돌릴 것** — CI가
  더 이상 안전망이 아니다. 수동 실행(`workflow_dispatch`)에서 `skip_tests`를 끄면 테스트를 돈다.
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
    일회성 스크립트를 SSH로 직접 실행해 변환했다 — 변환 내용과 백업 절차는 그
    커밋 메시지 참고(스크립트는 Python이라 이후 제거, 2026-09).
- career 세부 항목을 구조체 배열 → 단일 Markdown으로 (2026-09) — 편집 UX(회사 안에
  항목마다 title/period/bullets를 또 Accordion으로 접는 3중 중첩)가 싫다는 요청.
  `PortfolioCareerItem` 구조체 제거, `PortfolioCareerCompany.items: Vec<PortfolioCareerItem>`
  → `career_items_markdown: String`. 회사 스칼라 필드(company/period/employment_type/
  role/position)는 그대로. 운영 DB는 일회성 스크립트로 변환(항목별 `## {title}` +
  `_{period}_` + bullets를 빈 줄로 이어붙임, 멱등 — Python이라 이후 제거, 커밋 메시지 참고). tyange-cms
  에디터는 회사별 마크다운 필드 하나로, tyange-dev는 `company.career_items_markdown`를
  한 번 렌더하도록 동기화.
- featured_projects: `stack` 제거 + `folder` 도입 (2026-09) — `PortfolioProject`에서
  `stack: Vec<String>` 삭제(tyange-dev에서 더 이상 노출 안 함), `folder: String` 추가.
  같은 `folder` 값을 가진 프로젝트끼리 tyange-dev에서 한 묶음(라벨 아래 카드 그리드)으로
  렌더된다 — 예전엔 tyange-dev가 slug를 하드코딩해 그룹을 만들었다(`pickGrouped("tyange-cms")`).
  운영 DB는 `scripts/migrate_featured_folders.sh`(bash + jq + sqlite3)로 변환: `stack` 키
  삭제 + slug→folder 시드(tyange-cms/tyange-cms-api→"CMS 시스템", tyange-blog/tyange-dev→
  "개인 사이트", 나머지는 "" = 단독). 멱등. tyange-cms 에디터는 Stack(TagsInput) → Folder(텍스트) 교체.
  (마이그레이션 스크립트는 Python → bash로 전환 — 저장소에 Python 안 둠, 2026-09.)
- featured_projects: `summary` + `highlights_markdown` → `body_markdown` 하나로 병합 (2026-09) —
  편집 UX상 문단/불릿을 한 마크다운으로(career_items_markdown과 같은 방향). `PortfolioProject`에서
  두 필드 삭제, `body_markdown: String` 추가. 운영 DB는 `scripts/migrate_featured_body.sh`로
  변환: `trim(summary) + "\n\n" + trim(highlights_markdown)` (한쪽 비면 나머지만), 두 옛 키 삭제, 멱등.
  tyange-cms 에디터는 두 PortfolioMarkdownField → 하나로, tyange-dev는 `bodyHtml` 한 번 렌더.
