//! poketry(포켓몬 Z-A 엔트리 상성 분석) API 라우트.
//!
//! 이 파일은 아직 **가이드 주석만** 있습니다. 아래 D단계 → E단계 순서대로 직접 채우면 됩니다.
//! (선행: `poketry/src/analysis.rs` 의 B·C단계. 순서는 `poketry/src/lib.rs` 맨 위 표 참고)
//!
//! ════════════════════════════════════════════════════════════════════════════════
//! 만들 것: `POST /poketry/analyze`
//! ════════════════════════════════════════════════════════════════════════════════
//!
//! 요청 본문:
//! ```json
//! { "names": ["리자몽", "가디안", "루카리오", "갸라도스", "마기라스"] }
//! ```
//!
//! 응답(성공, 200) — cms-api 의 다른 라우트처럼 `ApiResponse` 로 감쌉니다:
//! ```json
//! {
//!   "status": true,
//!   "data": {
//!     "team":       [{ "name": "리자몽", "types": ["불꽃", "비행"], "capture_rate": 45 }, …],
//!     "weaknesses": [{ "attack_type": "노말", "weak_count": 0, "resist_count": 1 }, … 18줄],
//!     "weak_spots": ["바위", "전기"],
//!     "score": 12,
//!     "recommendations": [ …5마리일 때만, 포켓몬 3마리… ],
//!     "swaps": [ …6마리일 때만, { "remove": {포켓몬}, "add": {포켓몬} } 3개… ]
//!   },
//!   "message": null
//! }
//! ```
//!
//! 응답(실패, 400): 도감에 없는 이름 / 0마리 또는 7마리 이상.
//!
//! 💡 왜 GET 이 아니라 POST 인가?
//!    값을 바꾸지 않는 조회라 GET 이 원칙에 더 맞지만, 한글 이름 6개를 쿼리스트링에 넣으면
//!    URL 인코딩 때문에 curl 로 확인하기 번거롭고, 배열을 쿼리로 넘기는 규칙도 클라이언트마다
//!    다릅니다. JSON 본문이 단순해서 POST 로 정했습니다. GET 으로 바꾸고 싶다면
//!    `Query<String>` 으로 "리자몽,가디안" 을 받아 `split(',')` 하면 됩니다.
//!
//! ════════════════════════════════════════════════════════════════════════════════
//! D-1. 응답용 구조체(DTO) 만들기
//! ════════════════════════════════════════════════════════════════════════════════
//!
//! poem-openapi 의 `Json<T>` 로 돌려주려면 `T` 에 `#[derive(Object)]` 가 있어야 합니다.
//! (그래야 Swagger 문서(/api-docs)에 응답 모양이 자동으로 나옵니다)
//!
//! 그런데 `poketry::Pokemon`, `poketry::Weakness` 에는 `Object` 가 없습니다. 일부러 안 붙였어요:
//!   - poketry 가 poem-openapi 에 의존하면, 웹 프레임워크와 상관없는 계산 크레이트가
//!     프레임워크에 묶입니다. (나중에 CLI 나 다른 서버에서 다시 쓰기 어려워집니다)
//!   - API 로 보여 줄 모양과 계산용 모양은 다를 수 있습니다. 예를 들어 계산용
//!     `Pokemon` 은 `type1`/`type2: Option<Type>` 이지만, 프론트에는 `types: ["불꽃","비행"]`
//!     배열이 쓰기 편합니다. 필드 이름도 `twice_multiplier_count` 보다 `weak_count` 가 낫죠.
//!
//! 그래서 이 파일에 API 전용 구조체를 따로 만들고, poketry 값을 옮겨 담습니다.
//! `src/models.rs` 의 기존 구조체들이 좋은 예시입니다(`#[derive(Debug, Serialize, Object)]`).
//!
//! 만들 구조체 (이름은 바꿔도 됩니다):
//!
//! ```ignore
//! use poem_openapi::Object;
//!
//! #[derive(Debug, Object)]
//! pub struct AnalyzeRequest {
//!     pub names: Vec<String>,
//! }
//!
//! #[derive(Debug, Object)]
//! pub struct PokemonItem {
//!     pub name: String,
//!     pub types: Vec<String>,   // Type → String: `t.name().to_string()`
//!     pub capture_rate: u8,
//! }
//!
//! #[derive(Debug, Object)]
//! pub struct WeaknessRow { /* attack_type: String, weak_count: i32, resist_count: i32 */ }
//!
//! #[derive(Debug, Object)]
//! pub struct SwapItem { /* remove: PokemonItem, add: PokemonItem */ }
//! //  ⚠ `in` 은 Rust 예약어라 필드 이름으로 못 씁니다. 그래서 remove/add 로 했어요.
//!
//! #[derive(Debug, Object)]
//! pub struct AnalyzeResponse { /* 위 JSON 의 data 안쪽 필드 6개 */ }
//! ```
//!
//! 옮겨 담는 코드는 `From` 트레이트로 만들어 두면 깔끔합니다:
//!
//! ```ignore
//! impl From<&poketry::Pokemon> for PokemonItem {
//!     fn from(p: &poketry::Pokemon) -> Self {
//!         PokemonItem {
//!             name: p.name.clone(),
//!             types: p.types().iter().map(|t| t.name().to_string()).collect(),
//!             capture_rate: p.capture_rate,
//!         }
//!     }
//! }
//! // 쓰는 곳:  let items: Vec<PokemonItem> = team.iter().map(|p| PokemonItem::from(*p)).collect();
//! //   (`team` 은 `Vec<&Pokemon>` 이라 iter() 하면 `&&Pokemon` 이 나옵니다. `*p` 로 한 겹 벗겨요.
//! //    `.map(|&p| PokemonItem::from(p))` 처럼 패턴으로 벗겨도 됩니다.)
//! ```
//! `WeaknessRow` 도 `impl From<&poketry::Weakness> for WeaknessRow` 로 같은 모양입니다.
//!
//! ════════════════════════════════════════════════════════════════════════════════
//! D-2. 라우트 struct 와 핸들러
//! ════════════════════════════════════════════════════════════════════════════════
//!
//! `src/openapi.rs` 의 `TagsApi` 가 가장 단순한 예시입니다. 다른 Api 들은 DB 때문에
//! `state: Arc<AppState>` 를 들고 있지만, poketry 는 DB 를 안 쓰니 필드 없는 struct 면 됩니다.
//!
//! ```ignore
//! use poem::http::StatusCode;
//! use poem_openapi::{payload::Json, OpenApi};
//! use crate::models::ApiResponse;
//!
//! pub struct PoketryApi;
//!
//! #[OpenApi]
//! impl PoketryApi {
//!     /// 엔트리(1~6마리)의 타입 상성 분석. 5마리면 빈자리, 6마리면 교체 추천을 함께 줍니다.
//!     #[oai(path = "/poketry/analyze", method = "post")]
//!     async fn analyze(
//!         &self,
//!         body: Json<AnalyzeRequest>,
//!     ) -> poem::Result<Json<ApiResponse<AnalyzeResponse>>> {
//!         // ① 마릿수 검사: 비었거나 6마리 초과면 400
//!         // ② poketry::find_team(&body.names) → Err(UnknownPokemon) 이면 400
//!         // ③ poketry::team_weakness / weak_spot / team_score 로 계산
//!         // ④ 5마리면 poketry::recommend(&team, poketry::pokedex())
//!         //    6마리면 poketry::recommend_swap(&team, poketry::pokedex())
//!         //    그 외에는 빈 Vec
//!         // ⑤ DTO 로 옮겨 담아 Ok(Json(ApiResponse { status: true, data: Some(..), message: None }))
//!         todo!()
//!     }
//! }
//! ```
//!
//! 각 단계 힌트:
//!
//! ① / ② 400 응답 만들기 — 이 저장소의 다른 라우트와 같은 방식입니다:
//! ```ignore
//! return Err(poem::Error::from_string("1~6마리를 보내 주세요.", StatusCode::BAD_REQUEST));
//! ```
//! `find_team` 의 `Err` 는 `map_err` 로 바꾸고 `?` 로 바로 돌려보낼 수 있습니다:
//! ```ignore
//! let team = poketry::find_team(&body.names)
//!     .map_err(|e| poem::Error::from_string(e.to_string(), StatusCode::BAD_REQUEST))?;
//! ```
//! (`e.to_string()` 은 `UnknownPokemon` 의 `Display` 덕분에 "'xx' 은(는) 도감에 없습니다." 가 됩니다)
//!
//! `body.names` 를 바로 쓸 수 있는 이유: `Json<T>` 는 `Deref` 가 구현돼 있어서
//! `body.names` 가 `body.0.names` 와 같습니다.
//!
//! ③ 계산 함수들은 `&[&Pokemon]` 을 받습니다. `find_team` 이 `Vec<&'static Pokemon>` 을
//!    주니까 `&team` 으로 넘기면 됩니다.
//!
//! ④ `recommend` 결과는 `Vec<&Pokemon>`, `recommend_swap` 결과는 `Vec<(&Pokemon, &Pokemon)>`
//!    입니다. 튜플은 `.map(|(remove, add)| SwapItem { remove: remove.into(), add: add.into() })`
//!    처럼 풀어서 담으면 됩니다. (`From` 을 만들어 두면 `.into()` 도 공짜로 생깁니다)
//!
//! 생각해 볼 거리 (선택):
//!   - 같은 이름이 두 번 들어오면? 지금 poketry 는 막지 않습니다. 400 으로 막을지 정해 보세요.
//!   - 인증: 지금 설계는 누구나 호출 가능(공개)입니다. 관리자만 쓰게 하려면 `/admin/posts`
//!     처럼 핸들러 인자에 `guard: AuthGuard` 를 추가하고 `ensure_admin(&guard.0)?;` 를 부릅니다.
//!   - 계산량: `recommend_swap` 은 6 × (도감 수) 번 약점표를 만듭니다. 도감이 수백 마리라
//!     요청 하나에 수 ms 수준이라 async 핸들러 안에서 바로 계산해도 괜찮습니다.
//!   - CLI 시절의 "◀ 위험 / ◀ 받아 줄 팀원 없음" 표시를 `WeaknessRow` 에 넣고 싶다면
//!     `danger: Option<String>` 같은 필드를 추가해 보세요. 조건은 git 히스토리의
//!     옛 `src/main.rs` `print_team_weakness` 에 있습니다 (`git show db1a229^2:src/main.rs`).
//!
//! ════════════════════════════════════════════════════════════════════════════════
//! E. 등록하고 확인하기
//! ════════════════════════════════════════════════════════════════════════════════
//!
//! 1. `src/main.rs` 의 `OpenApiService::new((PostsApi {…}, TagsApi {…}, …), …)` 튜플 맨 끝에
//!    `PoketryApi,` 를 추가하고, 위쪽 `use` 에 `use poketry_api::PoketryApi;` 를 넣습니다.
//!    (필드 없는 struct 라 `PoketryApi {}` 가 아니라 `PoketryApi` 만 쓰면 됩니다)
//!
//! 2. 서버를 띄우고(`./dev.sh` 또는 `cargo run`) 확인:
//!    ```sh
//!    curl -s -X POST localhost:<포트>/poketry/analyze \
//!      -H 'Content-Type: application/json' \
//!      -d '{"names":["리자몽","가디안","루카리오","갸라도스","마기라스"]}' | jq
//!    ```
//!    포트는 `.env` / `main.rs` 의 서버 바인딩을 보세요. `/api-docs` 에서 Swagger 로도 해 볼 수 있습니다.
//!    실패도 확인: `{"names":["없는몬"]}` → 400, `{"names":[]}` → 400
//!
//! 3. (선택) 통합 테스트: `tests/` 나 `src/routes/*_test.rs` 의 기존 테스트처럼
//!    `poem::test::TestClient` 로 위 세 가지 요청을 테스트로 남겨 두면 좋습니다.
//!    이 저장소는 배포 전에 로컬에서 `cargo test` 를 돌리는 규칙이니(`.forgejo/workflows/deploy.yml`
//!    주석 참고), poketry 까지 포함하려면 `cargo test --workspace` 로 돌리세요.
//!
//! 4. tyange-cms-api 의 `CLAUDE.md` 규칙대로, 프론트(tyange-cms)에서 쓸 계획이 생기면
//!    그쪽 API 클라이언트도 맞춰 주세요.
