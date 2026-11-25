use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const EVOLUTIONS_JSON: &str = include_str!("../evolutions.json");

// TODO(C3단계): `evolutions.json` 을 읽어, 종 번호(`species_id`)로 그 포켓몬의 진화 계통을 찾게 하세요.
//
// 왜: GUI 에서 추천된 포켓몬을 보여줄 때, 진화 전부터 최종 진화형까지 이어진 계통을 통째로 보여주고
//   추천된 단계를 강조하려고 합니다. 상성 분석은 지금처럼 한 마리 단위이고, 이 모듈은 "보여주기"용입니다.
//
// 데이터: `poketry/evolutions.json` (수집 도구 `cargo run -p poketry-fetch` 가 만듭니다)
//   계통 179개의 배열이고, 계통 하나는 나무 모양입니다. 노드마다 다음 단계들이 `evolves_to` 에 들어 있어요.
//   ```json
//   [
//     { "species_id": 280, "name": "랄토스", "in_dex": true, "evolves_to": [
//         { "species_id": 281, "name": "킬리아", "in_dex": true, "evolves_to": [
//             { "species_id": 282, "name": "가디안",   "in_dex": true, "evolves_to": [] },
//             { "species_id": 475, "name": "엘레이드", "in_dex": true, "evolves_to": [] } ] } ] },
//     …
//   ]
//   ```
//   - 갈라지는 계통이 있습니다(이브이는 8갈래, 킬리아는 2갈래). 그래서 "줄"이 아니라 "나무"예요.
//   - 진화가 없는 포켓몬도 혼자인 계통으로 들어 있습니다(`evolves_to` 가 빈 배열).
//   - 메가진화는 나무에 없습니다. 메가는 진화 단계가 아니라 폼이라서, 도감(`pokedex()`)에서
//     같은 `species_id` 를 가진 줄로 찾습니다(메가리자몽X 도 `species_id` 6).
//   - `in_dex`: 이 게임 도감에 있는 종인지. 지금은 전부 true 지만 앞으로 false 가 생길 수 있어요.
//
// 1. 노드 구조체를 만드세요. (예: `EvolutionNode` — 이름은 자유)
//    - 필드는 위 JSON 키 네 개 그대로. 타입: `species_id: u32`, `name: String`, `in_dex: bool`,
//      `evolves_to` 는 "같은 구조체의 목록"입니다.
//    - 자기 자신을 필드로 갖는 구조체(재귀 구조체)는 보통 크기를 정할 수 없어서 에러가 나는데,
//      `Vec<자기>` 는 괜찮습니다. Vec 은 내용을 힙에 두고 자기 크기는 고정이라서요.
//      (`Option<자기>` 였다면 `Box` 로 감싸야 합니다 — 궁금하면 직접 해 보고 에러를 읽어 보세요)
//    - JSON 에서 읽어야 하니 `Deserialize`, API 로 내보내야 하니 `Serialize` 를 derive 하세요.
//      (`Pokemon` 구조체와 같은 방식: `use serde::{Deserialize, Serialize};`)
//
// 2. 내장 데이터 읽기 함수를 만드세요: `pub fn evolutions() -> &'static [EvolutionNode]`
//    `pokemon.rs` 의 `pokedex()` 와 똑같은 모양입니다. 그 함수를 열어 놓고 따라 쓰면 돼요.
//    - `include_str!("../evolutions.json")`: 컴파일할 때 파일 내용을 바이너리에 문자열로 넣습니다.
//      (경로는 이 파일 기준. 서버가 어느 폴더에서 실행되든 파일을 찾을 필요가 없어요)
//    - `OnceLock`: 처음 부를 때 한 번만 파싱하고, 그다음부터는 그 값을 돌려줍니다.
//      `static` 안에 들어 있어서 돌려주는 참조가 `'static` 이 됩니다.
//
// 3. 찾기 함수를 만드세요: `pub fn chain_of(species_id: u32) -> Option<&'static EvolutionNode>`
//    그 종이 들어 있는 계통의 "뿌리"(맨 처음 단계)를 돌려줍니다. 피카츄(25)를 넣으면 피츄(172) 노드.
//    - 나무 안에 어떤 번호가 있는지 보려면 노드 → `evolves_to` 의 각 노드 → … 를 끝까지 내려가야 합니다.
//      깊이가 정해져 있지 않으니 **재귀 함수**가 자연스럽습니다. 도우미를 하나 만들어 보세요:
//      `fn contains(node: &EvolutionNode, species_id: u32) -> bool`
//      "나 자신이 그 번호이거나, 내 다음 단계 중 하나가 그 번호를 contains 하면 true".
//      (힌트: `iter().any(...)` 를 이미 `recommend` 에서 써 봤어요)
//    - `chain_of` 는 계통 179개 중 `contains` 가 true 인 걸 찾으면 됩니다(`iter().find(...)`).
//    - 덤(선택): 매번 나무를 뒤지는 대신, 처음 한 번 `HashMap<u32, &EvolutionNode>`(번호 → 뿌리)를
//      만들어 `OnceLock` 에 넣어 두면 찾기가 바로 됩니다. 179개라 안 해도 충분히 빠릅니다.
//
// 4. `lib.rs` 의 `pub use` 에 새 함수·구조체를 추가해서 `poketry::chain_of` 로 쓸 수 있게 하세요.
//    (`pub mod evolution;` 은 이미 넣어 두었습니다)
//
// 5. 확인: 이 파일 맨 아래에 `#[cfg(test)] mod tests { use super::*; … }` 를 만들고 테스트를 써 보세요.
//    - 피카츄(25)의 계통 뿌리는 피츄(172)
//    - 이브이(133) 계통의 뿌리는 이브이 자신이고, 다음 단계가 8개
//    - 진화가 없는 종(예: 데덴네 702)은 뿌리가 자기 자신이고 다음 단계가 0개
//    - 도감의 모든 포켓몬은 계통이 있다: `pokedex()` 를 돌면서 `chain_of(p.species_id)` 가 전부 `Some`
//      (두 데이터 파일이 어긋나면 이 테스트가 잡아 줍니다 — 가장 중요한 테스트예요)
//    그다음 `cargo test -p poketry`.
#[derive(Debug, Serialize, Deserialize)]
pub struct EvolutionNode {
    pub species_id: u32,
    pub name: String,
    pub in_dex: bool,
    pub evolves_to: Vec<Self>,
}

impl EvolutionNode {
    pub fn contains(&self, species_id: u32) -> bool {
        self.species_id == species_id
            || self.evolves_to.iter().any(|next| next.contains(species_id))
    }
}

pub fn evolutions() -> &'static [EvolutionNode] {
    static EVOLUTIONS: OnceLock<Vec<EvolutionNode>> = OnceLock::new();
    EVOLUTIONS.get_or_init(|| {
        serde_json::from_str(EVOLUTIONS_JSON).expect("evolutions.json 형식이 잘못됐습니다.")
    })
}

pub fn chain_of(species_id: u32) -> Option<&'static EvolutionNode> {
    evolutions().iter().find(|root| root.contains(species_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pokemon::pokedex;

    #[test]
    fn 마지막_단계에서도_뿌리를_찾는다() {
        assert_eq!(chain_of(6).unwrap().species_id, 4);
        assert_eq!(chain_of(282).unwrap().species_id, 280);
    }

    #[test]
    fn 중간_단계에서도_뿌리를_찾는다() {
        assert_eq!(chain_of(25).unwrap().species_id, 172);
    }

    #[test]
    fn 갈라지는_계통은_다음_단계가_여럿이다() {
        let eevee = chain_of(133).unwrap();
        assert_eq!(eevee.species_id, 133);
        assert_eq!(eevee.evolves_to.len(), 8);
    }

    #[test]
    fn 진화가_없으면_자기_자신이_뿌리다() {
        let dedenne = chain_of(702).unwrap();
        assert_eq!(dedenne.species_id, 702);
        assert!(dedenne.evolves_to.is_empty());
    }

    #[test]
    fn 없는_번호는_none() {
        assert!(chain_of(99999).is_none());
    }

    #[test]
    fn 도감의_모든_포켓몬은_계통이_있다() {
        for p in pokedex() {
            assert!(
                chain_of(p.species_id).is_some(),
                "{} 의 계통이 없습니다",
                p.name
            );
        }
    }

    #[test]
    fn 메가는_원래_종의_계통에_있다() {
        let mega = pokedex().iter().find(|p| p.name == "메가리자몽X").unwrap();
        assert_eq!(chain_of(mega.species_id).unwrap().species_id, 4);
    }
}
