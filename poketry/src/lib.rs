//! # poketry — 포켓몬 레전드 Z-A 엔트리 타입 상성 분석
//!
//! 원래 `za-entry` 라는 CLI 였던 것을 tyange-cms-api 의 API 라우트에서 쓰는 라이브러리로
//! 바꾼 크레이트입니다. (git 히스토리는 `git subtree` 로 그대로 가져왔습니다)
//!
//! ## 모듈
//! - [`types`]    : 타입 18개와 상성표 (`effectiveness`)
//! - [`pokemon`]  : 포켓몬 구조체, 내장 도감(`pokedex()`), 이름 → 팀 변환(`find_team`)
//! - [`analysis`] : 팀 약점표, 보완할 타입, 점수, 빈자리/교체 추천
//!
//! ## 쓰는 쪽(cms-api 라우트)에서의 흐름
//! ```
//! use poketry::{find_team, pokedex, recommend, team_weakness, weak_spot};
//!
//! let team = find_team(&["리자몽", "가디안", "루카리오", "갸라도스", "마기라스"]).unwrap();
//! let table = team_weakness(&team);   // 공격 타입 18줄
//! let spots = weak_spot(&table);      // 보완할 타입
//! let picks = recommend(&team, pokedex()); // 5마리면 빈자리 추천
//! assert_eq!(table.len(), 18);
//! # let _ = (spots, picks);
//! ```
//!
//! ## CLI 에서 바뀐 점
//! - `main.rs`(인자 읽기·출력)와 `display.rs`(터미널 한글 폭 맞추기)는 지웠습니다.
//!   출력은 이제 JSON 이고, 모양은 cms-api 쪽이 정합니다.
//! - `pokemon.json` 은 파일로 읽지 않고 바이너리에 내장합니다(`pokemon.rs` 참고).
//! - 결과 구조체/타입에 `Serialize` 를 붙였습니다.
//!
//! ## 다음 스텝 (직접 작성)
//! 순서대로 하면 됩니다. 각 단계의 자세한 안내는 해당 위치의 `TODO` 주석에 있습니다.
//!
//! | 단계 | 어디 | 할 일 |
//! |------|------|------|
//! | B | `poketry/src/analysis.rs` `recommend`, `recommend_swap` | 정렬 3순위에 포획률 추가 |
//! | C | `poketry/src/analysis.rs` `recommend` | 구멍 수(`spot_count`) 계산 버그 점검 |
//! | C2 (계획) | `poketry/src/analysis.rs` | `recommend` 가 3마리가 아니라 후보 전체를 근거(구멍 수·점수 등)와 함께 순위대로 돌려주게 |
//! | C3 (계획) | `poketry/src/` 새 모듈 | `evolutions.json` 을 읽어 포켓몬 이름 → 진화 계통(나무)을 찾기 |
//! | D | cms-api `src/poketry_api.rs` | `POST /poketry/analyze` 라우트 작성 |
//! | E | cms-api `src/main.rs` | 라우트를 `OpenApiService` 에 등록하고 curl 로 확인 |
//!
//! 테스트 돌리기 (cms-api 루트에서):
//! - poketry 만: `cargo test -p poketry`
//! - 전부: `cargo test --workspace`  (그냥 `cargo test` 는 cms-api 만 돌립니다)

pub mod analysis;
pub mod pokemon;
pub mod types;

// 자주 쓰는 것들은 `poketry::이름` 으로 바로 쓸 수 있게 다시 내보냅니다.
pub use analysis::{
    Weakness, defense_multiplier, recommend, recommend_swap, team_score, team_weakness, weak_spot,
};
pub use pokemon::{Pokemon, UnknownPokemon, find, find_team, pokedex};
pub use types::{Type, effectiveness};
