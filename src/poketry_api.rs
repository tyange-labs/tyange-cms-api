// TODO(D단계): poketry(포켓몬 Z-A 엔트리 상성 분석) API 라우트 `POST /poketry/analyze` 를 만드세요.
//
// 이 파일에 응답 구조체(DTO)와 라우트를 씁니다. 서버에 등록하고 curl 로 확인하는 건 다음 단계(E)입니다.
// 이 단계에서는 `cargo build` 가 되면 끝이에요(아직 등록 전이라 "never used" 경고는 정상).
//
// ════════════════════════════════════════════════════════════════════════════════
// 만들 것: `POST /poketry/analyze`
// ════════════════════════════════════════════════════════════════════════════════
//
// 요청 본문:
// ```json
// { "names": ["리자몽", "가디안", "루카리오", "갸라도스", "마기라스"] }
// ```
//
// 응답(성공, 200) — cms-api 의 다른 라우트처럼 `ApiResponse` 로 감쌉니다:
// ```json
// {
//   "status": true,
//   "data": {
//     "team":       [{ "id": 6, "species_id": 6, "name": "리자몽", "types": ["불꽃", "비행"], "capture_rate": 45 }, …],
//     "weaknesses": [{ "attack_type": "노말", "weak_count": 0, "resist_count": 1 }, … 18줄],
//     "weak_spots": ["바위", "전기"],
//     "score": 12,
//     "recommendations": [ …5마리일 때만, { "pokemon": {포켓몬}, "spot_count": 2, "score": 14 } 를 좋은 순서대로… ],
//     "swaps": [ …6마리일 때만, { "remove": {포켓몬}, "add": {포켓몬}, "spot_count": 2, "score": 14 } 를 좋은 순서대로… ]
//   },
//   "message": null
// }
// ```
//
// 응답(실패, 400): 도감에 없는 이름 / 0마리 또는 7마리 이상.
//
// 💡 왜 GET 이 아니라 POST 인가?
//    값을 바꾸지 않는 조회라 GET 이 원칙에 더 맞지만, 한글 이름 6개를 쿼리스트링에 넣으면
//    URL 인코딩 때문에 curl 로 확인하기 번거롭고, 배열을 쿼리로 넘기는 규칙도 클라이언트마다
//    다릅니다. JSON 본문이 단순해서 POST 로 정했습니다. GET 으로 바꾸고 싶다면
//    `Query<String>` 으로 "리자몽,가디안" 을 받아 `split(',')` 하면 됩니다.
//
// ════════════════════════════════════════════════════════════════════════════════
// D-1. 응답용 구조체(DTO) 만들기
// ════════════════════════════════════════════════════════════════════════════════
//
// poem-openapi 의 `Json<T>` 로 돌려주려면 `T` 에 `#[derive(Object)]` 가 있어야 합니다.
// (그래야 Swagger 문서(/api-docs)에 응답 모양이 자동으로 나옵니다)
//
// 그런데 `poketry::Pokemon`, `poketry::Weakness` 에는 `Object` 가 없습니다. 일부러 안 붙였어요:
//   - poketry 가 poem-openapi 에 의존하면, 웹 프레임워크와 상관없는 계산 크레이트가
//     프레임워크에 묶입니다. (나중에 CLI 나 다른 서버에서 다시 쓰기 어려워집니다)
//   - API 로 보여 줄 모양과 계산용 모양은 다를 수 있습니다. 예를 들어 계산용
//     `Pokemon` 은 `type1`/`type2: Option<Type>` 이지만, 프론트에는 `types: ["불꽃","비행"]`
//     배열이 쓰기 편합니다. 필드 이름도 `twice_multiplier_count` 보다 `weak_count` 가 낫죠.
//
// 그래서 이 파일에 API 전용 구조체를 따로 만들고, poketry 값을 옮겨 담습니다.
// `src/models.rs` 의 기존 구조체들이 좋은 예시입니다(`#[derive(Debug, Serialize, Object)]`).
//
// 만들 구조체 (이름은 바꿔도 됩니다):
//
// ```ignore
// use poem_openapi::Object;
//
// #[derive(Debug, Object)]
// pub struct AnalyzeRequest {
//     pub names: Vec<String>,
// }
//
// #[derive(Debug, Object)]
// pub struct PokemonItem {
//     pub id: u32,              // GUI 가 고른 포켓몬을 다시 보낼 때는 이름 말고 이걸 쓰게 (이름은 표시용)
//     pub species_id: u32,      // 진화 계통(C3단계)을 찾을 때 쓰는 번호
//     pub name: String,
//     pub types: Vec<String>,   // Type → String: `t.name().to_string()`
//     pub capture_rate: u8,
// }
//
// #[derive(Debug, Object)]
// pub struct WeaknessRow { /* attack_type: String, weak_count: i32, resist_count: i32 */ }
//
// #[derive(Debug, Object)]
// pub struct RecommendationItem { /* pokemon: PokemonItem, spot_count: u32, score: i32 */ }
// //  poketry 의 `spot_count` 는 `usize` 인데, OpenAPI 스키마에는 `u32` 같은 고정 크기가 무난합니다(`as u32`).
//
// #[derive(Debug, Object)]
// pub struct SwapItem { /* remove: PokemonItem, add: PokemonItem, spot_count: u32, score: i32 */ }
// //  ⚠ `in` 은 Rust 예약어라 필드 이름으로 못 씁니다. 그래서 remove/add 로 했어요.
//
// #[derive(Debug, Object)]
// pub struct AnalyzeResponse { /* 위 JSON 의 data 안쪽 필드 6개 */ }
// ```
//
// 옮겨 담는 코드는 `From` 트레이트로 만들어 두면 깔끔합니다:
//
// ```ignore
// impl From<&poketry::Pokemon> for PokemonItem {
//     fn from(p: &poketry::Pokemon) -> Self {
//         PokemonItem {
//             id: p.id,
//             species_id: p.species_id,
//             name: p.name.clone(),
//             types: p.types().iter().map(|t| t.name().to_string()).collect(),
//             capture_rate: p.capture_rate,
//         }
//     }
// }
// // 쓰는 곳:  let items: Vec<PokemonItem> = team.iter().map(|p| PokemonItem::from(*p)).collect();
// //   (`team` 은 `Vec<&Pokemon>` 이라 iter() 하면 `&&Pokemon` 이 나옵니다. `*p` 로 한 겹 벗겨요.
// //    `.map(|&p| PokemonItem::from(p))` 처럼 패턴으로 벗겨도 됩니다.)
// ```
// `WeaknessRow` 도 `impl From<&poketry::Weakness> for WeaknessRow` 로 같은 모양입니다.
//
// ════════════════════════════════════════════════════════════════════════════════
// D-2. 라우트 struct 와 핸들러
// ════════════════════════════════════════════════════════════════════════════════
//
// `src/openapi.rs` 의 `TagsApi` 가 가장 단순한 예시입니다. 다른 Api 들은 DB 때문에
// `state: Arc<AppState>` 를 들고 있지만, poketry 는 DB 를 안 쓰니 필드 없는 struct 면 됩니다.
//
// ```ignore
// use poem::http::StatusCode;
// use poem_openapi::{payload::Json, OpenApi};
// use crate::models::ApiResponse;
//
// pub struct PoketryApi;
//
// #[OpenApi]
// impl PoketryApi {
//     /// 엔트리(1~6마리)의 타입 상성 분석. 5마리면 빈자리, 6마리면 교체 추천을 함께 줍니다.
//     #[oai(path = "/poketry/analyze", method = "post")]
//     async fn analyze(
//         &self,
//         body: Json<AnalyzeRequest>,
//     ) -> poem::Result<Json<ApiResponse<AnalyzeResponse>>> {
//         // ① 마릿수 검사: 비었거나 6마리 초과면 400
//         // ② poketry::find_team(&body.names) → Err(UnknownPokemon) 이면 400
//         // ③ poketry::team_weakness / weak_spot / team_score 로 계산
//         // ④ 5마리면 poketry::recommend(&team, poketry::pokedex())
//         //    6마리면 poketry::recommend_swap(&team, poketry::pokedex())
//         //    그 외에는 빈 Vec
//         // ⑤ DTO 로 옮겨 담아 Ok(Json(ApiResponse { status: true, data: Some(..), message: None }))
//         todo!()
//     }
// }
// ```
//
// 각 단계 힌트:
//
// ① / ② 400 응답 만들기 — 이 저장소의 다른 라우트와 같은 방식입니다:
// ```ignore
// return Err(poem::Error::from_string("1~6마리를 보내 주세요.", StatusCode::BAD_REQUEST));
// ```
// `find_team` 의 `Err` 는 `map_err` 로 바꾸고 `?` 로 바로 돌려보낼 수 있습니다:
// ```ignore
// let team = poketry::find_team(&body.names)
//     .map_err(|e| poem::Error::from_string(e.to_string(), StatusCode::BAD_REQUEST))?;
// ```
// (`e.to_string()` 은 `UnknownPokemon` 의 `Display` 덕분에 "'xx' 은(는) 도감에 없습니다." 가 됩니다)
//
// `body.names` 를 바로 쓸 수 있는 이유: `Json<T>` 는 `Deref` 가 구현돼 있어서
// `body.names` 가 `body.0.names` 와 같습니다.
//
// ③ 계산 함수들은 `&[&Pokemon]` 을 받습니다. `find_team` 이 `Vec<&'static Pokemon>` 을
//    주니까 `&team` 으로 넘기면 됩니다.
//
// ④ `recommend` 결과는 `Vec<poketry::Recommendation>`(`candidate`·`spot_count`·`score`),
//    `recommend_swap` 결과는 `Vec<poketry::Swap>`(`out`·`candidate`·`spot_count`·`score`)입니다(C2단계).
//    후보 **전체**가 좋은 순서대로 오고 개수 제한이 없습니다 — 몇 개를 줄지는 이 라우트가 정합니다
//    (예: `.into_iter().take(20)`). 담을 때는 필드 이름으로 꺼내면 됩니다:
//    `.map(|s| SwapItem { remove: s.out.into(), add: s.candidate.into(), … })`
//    (`From<&Pokemon>` 을 만들어 두면 `.into()` 도 공짜로 생깁니다. 두 결과 구조체에도 `From` 을 만들면 더 깔끔합니다)
//
// 생각해 볼 거리 (선택):
//   - 개수: 위에서 고정으로 자른 개수를 요청 본문의 `limit: Option<u32>` 로 받게 할 수도 있습니다.
//   - 요청을 이름 대신 `ids: Vec<u32>` 로 받기: GUI 가 붙으면 그쪽이 안전합니다(CLAUDE.md "이름이 아니라 id로 잇는다").
//     지금은 curl 로 확인하기 쉬운 이름으로 시작하고, 나중에 둘 다 받게 해도 됩니다.
//   - 같은 이름이 두 번 들어오면? 지금 poketry 는 막지 않습니다. 400 으로 막을지 정해 보세요.
//   - 인증: 지금 설계는 누구나 호출 가능(공개)입니다. 관리자만 쓰게 하려면 `/admin/posts`
//     처럼 핸들러 인자에 `guard: AuthGuard` 를 추가하고 `ensure_admin(&guard.0)?;` 를 부릅니다.
//   - 계산량: `recommend_swap` 은 6 × (도감 수) 번 약점표를 만듭니다. 도감이 수백 마리라
//     요청 하나에 수 ms 수준이라 async 핸들러 안에서 바로 계산해도 괜찮습니다.
//   - CLI 시절의 "◀ 위험 / ◀ 받아 줄 팀원 없음" 표시를 `WeaknessRow` 에 넣고 싶다면
//     `danger: Option<String>` 같은 필드를 추가해 보세요. 조건은 git 히스토리의
//     옛 `src/main.rs` `print_team_weakness` 에 있습니다 (`git show db1a229^2:src/main.rs`).
//   - 진화 계통: GUI 에서 추천된 포켓몬의 계통을 보여주려면 `poketry::chain_of(species_id)` 가
//     뿌리 노드(`EvolutionNode`, 나무 모양)를 줍니다. 응답에 넣는 방법은 둘 — 추천 항목마다 계통을 붙이거나,
//     `GET /poketry/chain/{species_id}` 같은 라우트를 따로 두거나. 따로 두는 쪽이 응답이 작고 단순합니다.
//     (나무 모양 DTO 를 `Object` 로 만들 때는 poem-openapi 가 자기 자신을 품는 구조체를 받는지 먼저 확인해 보세요)
