# tyange-cms-api

Rust + Poem + SQLite로 만든 개인용 CMS API 서버입니다.
블로그 콘텐츠를 관리하기 위해 만든 백엔드이며 현재는 단일 운영자(개인) 사용 시나리오를 중심으로 구성되어 있습니다.

## 이 프로젝트의 목적

이 API는 크게 다음을 해결하기 위해 만들어졌습니다.

- 블로그 운영: 포스트/태그/이미지/포트폴리오 데이터를 CMS 형태로 관리
- 자동 재배포: 포스트 변경 시 `tyange-blog`를 Forgejo Actions로 재배포 트리거

즉, "공용 SaaS형 CMS"보다는 "개인 콘텐츠 + 개인 인프라 연동"에 최적화된 구조입니다.

## Tech Stack

- Rust (Edition 2021)
- Poem
- SQLx (SQLite, 컴파일 타임 쿼리 검증)
- Tokio
- JWT (`jsonwebtoken`)
- bcrypt
- `reqwest` (Forgejo `workflow_dispatch` API 호출)

## 빠른 시작

```bash
./dev.sh
```

`.env`가 없으면 개발용 더미 JWT 시크릿으로 하나 만들고 `cargo run -q`를 실행한다.
이미 `.env`가 있으면 손대지 않는다. 그 외 환경변수까지 필요하면 아래를 따라 직접 설정한다.

### 1) 환경변수 설정

프로젝트 루트 `.env`에 아래 값을 설정하세요.

```env
# 상대 경로 예시
DATABASE_PATH=./data/database.db
UPLOAD_PATH=.uploads/images
UPLOAD_MAX_BYTES=20971520

# JWT
JWT_ACCESS_SECRET=replace-with-access-secret
```

`DATABASE_PATH`, `UPLOAD_PATH`는 절대 경로로 직접 지정해도 됩니다.

```env
# 절대 경로 예시
DATABASE_PATH=/Users/yourname/data/tyange/database.db
UPLOAD_PATH=/Users/yourname/data/tyange/uploads/images
UPLOAD_MAX_BYTES=20971520
```

### 패스키 환경변수 (선택)

`PASSKEY_RP_ID`가 비어 있으면 패스키 기능 전체가 꺼진다(시작 로그에 이유를 남김).
`GOTOLIB_MAX_ACCOUNTS`(기본 1000)는 gotolib 전용 계정의 총량 상한이다.

```env
# RP ID는 등록 가능한 도메인. tyange.com이면 그 아래 서브도메인 어디서든 같은 패스키를 쓴다.
PASSKEY_RP_ID=tyange.com
# 패스키 의식을 시작하는 페이지의 origin. 쉼표로 여러 개, 첫 번째가 기본값.
PASSKEY_RP_ORIGIN=https://gotolib.tyange.com
# PASSKEY_RP_NAME=tyange
```

로컬 개발은 `PASSKEY_RP_ID=localhost`, `PASSKEY_RP_ORIGIN=http://localhost:5199`처럼 맞춘다.
origin의 호스트는 RP ID와 같거나 그 하위 도메인이어야 한다.

### 블로그 자동 재배포 환경변수 (선택)

포스트 publish/update/delete 후 `tyange-blog`를 재배포하려면 아래를 설정합니다.
`TYANGE_BLOG_REDEPLOY_TOKEN`이 비어 있으면 이 기능 전체가 조용히 꺼집니다(로그만 남김).

```env
TYANGE_BLOG_REDEPLOY_TOKEN=replace-with-forgejo-api-token
# 아래는 기본값이 있어 보통 설정할 필요 없음
# TYANGE_BLOG_REDEPLOY_REPO=tyange-labs/tyange-blog
# TYANGE_BLOG_REDEPLOY_API_BASE=https://git.tyange.com/api/v1
# TYANGE_BLOG_REDEPLOY_WORKFLOW_FILENAME=deploy.yml
# TYANGE_BLOG_REDEPLOY_REF=master
```

`TYANGE_BLOG_REDEPLOY_TOKEN`은 GitHub PAT가 아니라 `tyange-labs/tyange-blog` 저장소의
Actions 실행 권한을 가진 **Forgejo API 토큰**이어야 합니다.

### 2) 실행

```bash
cargo run -q
```

기본 바인드 주소는 `0.0.0.0:8080` 입니다.

## 인증

### JWT 인증 (`Authorization`)

`Authorization` 헤더에 JWT 액세스 토큰을 넣어 호출합니다.
관리자/작성자 권한이 필요한 CMS 수정 계열 API에서 사용됩니다.

## API 개요

### Health

- `GET /health`
서버 생존 확인용 헬스체크.

- `GET /health-check`
서버 생존 확인용 대체 헬스체크.

CORS preflight(OPTIONS)는 `Cors` 미들웨어가 전담한다. 별도의 캐치올 라우트는 없다
(poem-openapi 도입 시 `/*path` 캐치올이 OpenApiService의 root nest와 충돌해서 제거함).

### Auth

- `POST /login`
사용자 로그인 후 access 토큰 발급(exp 1년, refresh token 없음).

- `POST /admin/add-user` (JWT)
신규 사용자 계정 추가(비밀번호 해시 저장). 공개 회원가입(`POST /signup`)은 없다 — 계정은 관리자가
만든다(2026-10 제거).

### Passkeys (WebAuthn)

비밀번호 로그인과 같은 JWT를 발급하는 패스키 로그인. 사용자명 없이(discoverable) 로그인한다.
`PASSKEY_RP_ID`가 없으면 아래 엔드포인트는 전부 503이다(환경변수는 위 "패스키 환경변수" 참고).

- `POST /passkeys/signup/start` → `{ challenge_id, options }`, `POST /passkeys/signup/finish`
  `{ challenge_id, label?, credential }` → `{ access_token, user_role: "gotolib" }` — **가입**.
  이메일·비밀번호 없이 패스키 하나로 gotolib 전용 계정(`user_id = gl-<uuid>`, 역할 `gotolib`)을
  만든다. 계정은 finish에서 검증이 끝난 뒤에야 DB에 생긴다. 누구나 부를 수 있어서 IP당 시간당
  start 10회·finish 5회로 제한하고, 계정 총량 상한(`GOTOLIB_MAX_ACCOUNTS`, 기본 1000)에 닿으면 503.
  365일 동안 활동(패스키 사용·동기화)이 없는 gotolib 계정은 하루 한 번 정리한다.
- `POST /passkeys/register/start` (JWT) → `{ challenge_id, options }` — 이 기기에 패스키 등록 시작.
  `options`는 `navigator.credentials.create({ publicKey })`에 넘기는 값(`options.publicKey`).
- `POST /passkeys/register/finish` (JWT) `{ challenge_id, label?, credential }` → `{ id, label }`
- `POST /passkeys/login/start` → `{ challenge_id, options }` — 인증 불필요, IP당 분당 20회.
- `POST /passkeys/login/finish` `{ challenge_id, credential }` → `{ access_token, user_role }`
- `GET /passkeys` (JWT) 내 패스키 목록, `DELETE /passkeys/:id` (JWT) 삭제(내 것만)

`challenge_id`는 서버가 start에서 만든 상태를 가리키는 일회용 키다(5분, 한 번 쓰면 폐기).
`credential`은 브라우저의 `PublicKeyCredential.toJSON()` 모양 그대로 보낸다.

### gotolib 동기화

- `GET /gotolib/sync` (JWT) → `{ version, data, updated_at }` (처음이면 `version: 0, data: null`).
  `ETag: "v<version>"`을 붙이고 `If-None-Match`가 같으면 본문 없이 304.
- `PUT /gotolib/sync` (JWT) `{ base_version, data }` — `data`는 JSON 객체(512KB 이하). `base_version`이
  서버의 현재 version과 같을 때만 저장하고 version을 1 올린다. 다르면 409와 함께 최신 문서를
  돌려주므로 클라이언트가 합쳐서 다시 올린다. 서버는 내용을 해석하지 않는다.

### Posts / Tags (CMS)

- `GET /posts`
공개용 포스트 목록 조회(초안 제외), 작성자 필터 지원.

- `GET /posts/search-with-tags`
포함/제외 태그 조건으로 포스트 검색.

- `GET /post/:post_id`
단일 포스트 상세 조회.

- `POST /post/upload` (JWT)
새 포스트 작성 및 태그 연결.
공개 상태(`status != draft`)이면서 `dev` 태그가 없으면 커밋 직후 `tyange-blog` rebuild trigger를 보낸다.

- `PUT /post/update/:post_id` (JWT)
본인 포스트 내용/태그 수정.
blog 대상 포스트 판정은 `status != draft` 이고 `dev` 태그가 없는 경우다. 이 기준으로 draft에서 공개로 전환되거나, 공개 필드가 바뀌거나, `dev` 태그 추가/삭제 때문에 blog 포함 여부가 바뀌면 커밋 직후 `tyange-blog` rebuild trigger를 보낸다.

- `DELETE /post/delete/:post_id` (JWT)
본인 포스트 삭제.
삭제 전 blog 대상 포스트였던 경우만 커밋 직후 `tyange-blog` rebuild trigger를 보낸다.

- `GET /admin/posts` (JWT)
관리자용 전체 포스트 목록 조회(초안 포함).

- `GET /tags`
태그별 사용 횟수 조회(카테고리 필터 가능).

- `GET /tags-with-category`
카테고리별 태그 묶음 조회.

### Images / Portfolio

- `POST /upload-image` (JWT) / `POST /images/upload` (JWT)
이미지 업로드 후 웹 경로(`/images/...`) 반환. 두 경로 모두 동일 핸들러.

- `GET /portfolio`
포트폴리오 콘텐츠 조회. DB 스키마는 `portfolio_section` 테이블로 분리되어 있지만, 응답은 기존 단일 문서 형태로 병합되어 호환성을 유지한다.

- `PUT /portfolio/update` (JWT, Admin)
포트폴리오 콘텐츠 전체 수정. 최초 생성 시에만 쓰고 이후는 섹션별 저장(`/portfolio/sections/:section_key`)을 쓴다. (`PUT /portfolio`라는 경로는 없다 — 과거 문서에 오기재돼 있었음.)

- `PUT /portfolio/sections/:section_key` (JWT, Admin)
포트폴리오 섹션(`intro`, `career`, `featured_projects` 등)만 부분 업데이트. 단일 JSON을 통째로 덮어쓰지 않고 섹션 단위로 저장해 동시 편집 충돌과 요청 크기를 줄인다.

**Markdown 필드**: `intro.content`, `featured_projects[].body_markdown`,
`career.companies[].career_items_markdown`은 markdown 문자열이다. 백엔드는 이 필드들을
파싱하지 않고 문자열 그대로 저장·반환만 한다 — 렌더링은 프론트엔드(tyange-cms 미리보기,
tyange-dev)가 담당한다. `body_markdown`은 예전 `summary`(문단) + `highlights_markdown`
(불릿 리스트)을 합친 필드, `career_items_markdown`은 예전
`items: { title, period, bullets }[]` 구조체 배열을 대체한 필드다(둘 다 2026-09).
`featured_projects[].folder`는 같은 값끼리 tyange-dev에서 한 묶음으로 묶어 렌더하는 필드로,
같은 시점에 제거된 `stack: string[]`을 대체했다.

- `DELETE /portfolio` (JWT, Admin)
포트폴리오 초기화.

### Me / Profile

- `GET /me` (JWT) — 현재 로그인 사용자 정보.
- `PUT /me/profile` (JWT) — 프로필 수정 (display_name, avatar, bio 등).

### Wedding Guests

lovelog(모바일 청첩장) RSVP 연동. 하객 실명이 담기므로 조회는 반드시 인증을 유지한다.

- `POST /public/wedding/guests` — 인증 없는 공개 쓰기(RateLimit: IP당 5회/분 + 30회/시간, 본문 4KB 상한). `name`(trim 후 1~20자), `companions`(0~10, 기본 0), `visit_hour`(필수, 12~16 — 전시 운영시간 12~17시를 한 시간 단위로 끊은 시작 시각).
- `GET /admin/wedding/guests` (JWT, Admin) — 하객 목록 조회.

## 테스트

```bash
cargo test
```

게시글 테스트에는 `dev` 태그 제외 규칙을 반영한 publish/update/delete trigger 조건과, dispatch 실패가 있어도 CMS 저장은 유지되는지가 포함됩니다.

## curl 예시

### 로그인 후 JWT 획득

```bash
curl -sS -X POST http://127.0.0.1:8080/login \
  -H 'Content-Type: application/json' \
  -d '{"user_id":"me@example.com","password":"secret"}'
```

## 설계 메모

- **권한 스코핑**: 모든 수정/조회 API는 미들웨어로 `current_user()`를 주입받아 `user_id` 소유권을 일관되게 검증한다. 관리자 전용 엔드포인트는 `AdminOnly` 미들웨어가 role까지 확인한다.
- **포트폴리오 DB 마이그레이션**: 초기에는 `portfolio_documents`에 단일 JSON을 저장했으나, 섹션 단위 편집 UX가 필요해지면서 `portfolio_section(section_key, content)` 테이블로 분리. 응답 스키마는 기존 클라이언트 호환성을 위해 런타임에 섹션들을 병합해 단일 문서로 내린다. `portfolio_section.content`는 각 섹션의 JSON을 그대로 문자열로 담는 TEXT 컬럼(컬럼명은 `content_json`이 아니라 `content`).
- **포트폴리오 markdown 필드 검증**: `PUT /portfolio/sections/:key`는 섹션 키마다 다른 구조체로 파싱해서 검증한 뒤, **파싱된 구조체를 재직렬화**해 저장한다(원본 JSON을 그대로 저장하지 않음) — 그래야 스키마에 없는 키가 DB에 남지 않는다. markdown 필드(`intro.content` 등) 자체는 여전히 파싱 없이 문자열로만 검증된다.
- **블로그 재배포 트리거**: 포스트 publish/update/delete 후 `dev` 태그가 없는 경우에 한해 `tyange-blog` 저장소의 Forgejo `workflow_dispatch` API(`POST /repos/{owner}/{repo}/actions/workflows/{file}/dispatches`, `Authorization: token <TOKEN>`)를 호출해 블로그를 재배포한다. dispatch 호출 실패는 로그만 남기고 CMS 저장은 유지한다. (2026-09 이전에는 GitHub `repository_dispatch`를 썼으나, Forgejo엔 대응 기능이 없어 이식했다.)

## 테스트 커버리지 하이라이트

- `portfolio_routes_test.rs` — 포트폴리오 섹션 분리 이후 응답 호환성 및 부분 업데이트 검증.
- `post_authorization_test.rs` — 소유권 스코핑·권한 경계 회귀 방지.
