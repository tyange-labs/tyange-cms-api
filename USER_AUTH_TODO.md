# User Auth TODO

현재 구현된 범위:

- ~~`POST /signup`~~ — 제거(2026-10). 계정은 `POST /admin/add-user`로만 만든다
- `POST /login`
- `GET /me`
- `POST /admin/add-user`

남은 작업:

- Add `PUT /me/password`
  - Require current password verification
  - Enforce password policy and bcrypt re-hash

- Add `DELETE /me`
  - Define deletion policy for owned `posts` and `images`
  - Decide whether admin accounts can self-delete

~~Add refresh-token flow~~ — 결정 완료(2026-09): 반대 방향으로 정리했다. refresh token은
검증 엔드포인트(`/refresh`)가 없어 dead였고, 구현하는 대신 아예 제거했다
(`src/auth/jwt.rs` 참고). access token exp를 1년으로 늘려 세션을 유지한다
(tyange-cms CLAUDE.md `## 로그인 > 세션 유지` 참고).

~~Add logout strategy~~ — 위 결정에 따라 자동으로 답이 나옴: 서버에 무효화할 refresh
token이 없으니 logout은 순수 클라이언트 사이드(토큰 폐기)뿐이다.

- Consider renaming login/signup identifiers
  - Current login uses `user_id` field but semantically expects email
  - Decide whether to keep compatibility or move to explicit `email`

- Strengthen auth validation
  - Normalize email input
  - Standardize auth error responses
  - Revisit password policy beyond minimum length

- Add admin user-management APIs
  - `GET /admin/users`
  - `GET /admin/users/:user_id`
  - `PUT /admin/users/:user_id`
  - `DELETE /admin/users/:user_id`

- Expand auth/account tests
  - `/me` unauthorized access
  - Admin login success path
  - Password change invalidates old password
  - Deleted account cannot log in
  - `/me` response for both `user` and `admin`
