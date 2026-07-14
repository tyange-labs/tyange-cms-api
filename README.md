# tyange-cms-api

Rust + Poem + SQLite로 만든 개인용 CMS API 서버입니다.
블로그 콘텐츠를 관리하기 위해 만든 백엔드이며 현재는 단일 운영자(개인) 사용 시나리오를 중심으로 구성되어 있습니다.

## 이 프로젝트의 목적

이 API는 크게 다음을 해결하기 위해 만들어졌습니다.

- 블로그 운영: 포스트/태그/이미지/포트폴리오 데이터를 CMS 형태로 관리
- 알림 허브: 구독한 RSS 소스의 새 글을 감지하고 구독 브라우저로 Web Push 전송

즉, "공용 SaaS형 CMS"보다는 "개인 콘텐츠 + 개인용 알림"에 최적화된 구조입니다.

## Tech Stack

- Rust (Edition 2021)
- Poem
- SQLx (SQLite, 컴파일 타임 쿼리 검증)
- Tokio
- JWT (`jsonwebtoken`)
- bcrypt
- `web-push` (VAPID Web Push 발송)
- `feed-rs` (RSS/Atom 파싱 워커)
- `reqwest` (Google `tokeninfo`, GitHub `repository_dispatch` 호출)

## 빠른 시작

### 1) 환경변수 설정

프로젝트 루트 `.env`에 아래 값을 설정하세요.

```env
# 상대 경로 예시
DATABASE_PATH=./data/database.db
UPLOAD_PATH=.uploads/images
UPLOAD_MAX_BYTES=20971520

# JWT
JWT_ACCESS_SECRET=replace-with-access-secret
JWT_REFRESH_SECRET=replace-with-refresh-secret

# Google Login
GOOGLE_CLIENT_ID=replace-with-google-oauth-client-id

# Web Push (RSS 기반 브라우저 푸시)
VAPID_PUBLIC_KEY=replace-with-vapid-public-key
VAPID_PRIVATE_KEY=replace-with-vapid-private-key
VAPID_SUBJECT=mailto:you@example.com

```

`DATABASE_PATH`, `UPLOAD_PATH`는 절대 경로로 직접 지정해도 됩니다.

```env
# 절대 경로 예시
DATABASE_PATH=/Users/yourname/data/tyange/database.db
UPLOAD_PATH=/Users/yourname/data/tyange/uploads/images
UPLOAD_MAX_BYTES=20971520
```

### Web Push 환경변수

RSS 기반 브라우저 푸시를 사용하려면 아래 환경변수를 함께 설정해야 합니다.

- `VAPID_PUBLIC_KEY`: `GET /push/public-key`가 대시보드에 공개하는 브라우저 등록용 공개키
- `VAPID_PRIVATE_KEY`: 실제 Web Push 발송 시 VAPID 서명을 만드는 비공개키
- `VAPID_SUBJECT`: Web Push VAPID `sub` claim에 넣는 연락처. 일반적으로 `mailto:...` 또는 HTTPS URL

동작 방식은 다음과 같습니다.

- `VAPID_PUBLIC_KEY`가 없거나 빈 문자열이면 `GET /push/public-key`는 `503 Service Unavailable`을 반환합니다.
- `VAPID_PUBLIC_KEY`, `VAPID_PRIVATE_KEY`, `VAPID_SUBJECT`가 모두 있어야 RSS polling worker가 실제 브라우저 푸시를 전송할 수 있습니다.

로컬 개발 예시는 아래와 같습니다.

```env
VAPID_PUBLIC_KEY=your-generated-public-key
VAPID_PRIVATE_KEY=your-generated-private-key
VAPID_SUBJECT=mailto:dev@example.com
```

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

- `OPTIONS /*path`
CORS preflight 처리.

### Auth

- `POST /login`
사용자 로그인 후 access/refresh 토큰 발급.

- `POST /login/google`
프론트엔드가 Google Sign-In 후 받은 `id_token`을 전달하면, 서버가 토큰을 검증한 뒤 access/refresh 토큰을 발급합니다.
동일 이메일의 기존 로컬 계정이 있으면 해당 계정에 Google 로그인을 연결합니다.

- `POST /admin/add-user` (JWT)
신규 사용자 계정 추가(비밀번호 해시 저장).

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

- `PUT /portfolio` / `PUT /portfolio/update` (JWT, Admin)
포트폴리오 콘텐츠 전체 수정. `/portfolio/update`는 레거시 경로.

- `PUT /portfolio/sections/:section_key` (JWT, Admin)
포트폴리오 섹션(`intro`, `career`, `featured_projects` 등)만 부분 업데이트. 단일 JSON을 통째로 덮어쓰지 않고 섹션 단위로 저장해 동시 편집 충돌과 요청 크기를 줄인다.

- `DELETE /portfolio` (JWT, Admin)
포트폴리오 초기화.

### Me / Profile

- `GET /me` (JWT) — 현재 로그인 사용자 정보.
- `PUT /me/profile` (JWT) — 프로필 수정 (display_name, avatar, bio 등). Google 로그인 직후 기본값 자동 병합.

### RSS 구독 / Feed / Web Push

- `GET /rss-sources` (JWT) — 내가 구독한 RSS 소스 목록.
- `POST /rss-sources` (JWT) — RSS 소스 추가(중복 URL은 기존 소스와 공유 + 구독 레코드만 생성).
- `DELETE /rss-sources/:source_id/subscription` (JWT) — 구독 해지(소스는 다른 유저가 구독 중이면 유지).
- `GET /feed/items` (JWT) — 구독 소스들의 최신 feed 아이템을 오프셋 페이지네이션으로 조회(`limit`, `offset`).
- `GET /push/public-key` — VAPID 공개키 반환. 환경변수가 비어 있으면 503.
- `GET /push/subscriptions` (JWT) — 현재 유저의 Web Push 구독 목록.
- `POST /push/subscriptions` (JWT) — 브라우저의 PushSubscription upsert.
- `DELETE /push/subscriptions` (JWT) — 구독 해제.

백그라운드 RSS 폴링 워커(`rss_push.rs`)가 주기적으로 새 feed 아이템을 감지하고, 구독 브라우저의 PushSubscription으로 VAPID 서명된 Web Push를 발송한다.

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

### Google 로그인 후 JWT 획득

```bash
curl -sS -X POST http://127.0.0.1:8080/login/google \
  -H 'Content-Type: application/json' \
  -d '{"id_token":"GOOGLE_ID_TOKEN_FROM_FRONTEND"}'
```

## 설계 메모

- **권한 스코핑**: 모든 수정/조회 API는 미들웨어로 `current_user()`를 주입받아 `user_id` 소유권을 일관되게 검증한다. 관리자 전용 엔드포인트는 `AdminOnly` 미들웨어가 role까지 확인한다.
- **포트폴리오 DB 마이그레이션**: 초기에는 `portfolio_documents`에 단일 JSON을 저장했으나, 섹션 단위 편집 UX가 필요해지면서 `portfolio_section(section_key, content_json)` 테이블로 분리. 응답 스키마는 기존 클라이언트 호환성을 위해 런타임에 섹션들을 병합해 단일 문서로 내린다.
- **블로그 재배포 트리거**: 포스트 publish/update/delete 후 `dev` 태그가 없는 경우에 한해 `tyange-blog` 저장소의 GitHub `repository_dispatch`(`event_type=cms-content-changed`)를 호출해 블로그를 재배포한다. dispatch 호출 실패는 로그만 남기고 CMS 저장은 유지한다.
- **Web Push 옵셔널 구동**: VAPID 환경변수가 모두 설정되어야 RSS 폴링 워커가 브라우저 푸시를 실제 발송한다. 부분 설정이면 공개키 엔드포인트가 503으로 응답하고 워커는 아이템 저장만 수행한다.

## 테스트 커버리지 하이라이트

- `portfolio_routes_test.rs` — 포트폴리오 섹션 분리 이후 응답 호환성 및 부분 업데이트 검증.
- `feed_items_test.rs`, `post_authorization_test.rs` — 소유권 스코핑·권한 경계 회귀 방지.
