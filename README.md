# tyange-cms-api

개인 블로그·포트폴리오를 관리하는 CMS 백엔드. gotolib(도서관 앱)·lovelog 같은 개인 서비스의 API도 함께 제공한다.

- Rust + [Poem](https://github.com/poem-web/poem) / poem-openapi
- SQLite + SQLx
- `poketry/`: 포켓몬 레전드 Z-A 팀 타입 상성 분석 라이브러리 (워크스페이스 멤버, `POST /poketry/analyze`)

## 실행

```bash
cargo run
```

루트 `.env`에 최소한 아래를 둔다:

```env
JWT_ACCESS_SECRET=change-me
DATABASE_PATH=./data/database.db   # 기본값
UPLOAD_PATH=.uploads/images        # 기본값
LISTEN_ADDR=0.0.0.0:8080           # 기본값
```

그 밖의 기능(패스키, Claude 커넥터(`MCP_PUBLIC_URL`), gotolib 알림 등)은 해당 환경변수가 없으면 꺼진 채로 뜬다.

## API 문서

서버를 띄운 뒤 `/api-docs`(Swagger UI), 스펙은 `/api-docs/openapi.json`.

## 테스트

```bash
cargo test --workspace   # poketry 포함
```
