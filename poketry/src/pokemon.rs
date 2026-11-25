//! 포켓몬 구조체와 도감 데이터. (Claude가 만든 뼈대)
//!
//! CLI 시절에는 실행할 때마다 `pokemon.json` 을 파일로 읽었는데, API 서버는 어느 폴더에서
//! 실행될지 모르고 요청마다 파일을 다시 읽을 이유도 없습니다. 그래서 JSON 을 컴파일할 때
//! 바이너리에 넣고(`include_str!`), 처음 한 번만 파싱해서 프로그램이 끝날 때까지 씁니다.

use crate::types::Type;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::OnceLock;

/// 컴파일할 때 바이너리에 들어가는 도감 원본. 경로는 이 파일(src/pokemon.rs) 기준입니다.
const POKEDEX_JSON: &str = include_str!("../pokemon.json");

/// 포켓몬 한 마리. 단일 타입이면 `type2` 가 `None` 입니다.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Pokemon {
    /// 이 줄(폼)의 번호 — PokeAPI 의 pokemon id. 리자몽 6, 메가리자몽X 10034.
    /// 이름은 바뀔 수 있으니(PokeAPI 에 한국어 이름이 새로 생기는 등) 저장하거나 API 로 주고받을 때는 이걸 씁니다.
    pub id: u32,
    /// 종 번호(전국도감 번호). 메가 폼은 원래 종과 같은 값입니다 — 메가리자몽X 도 6.
    /// `evolutions.json` 의 진화 계통 노드가 이 번호로 도감과 이어집니다.
    pub species_id: u32,
    /// 표시용 한국어 이름.
    pub name: String,
    pub type1: Type,
    #[serde(default)]
    pub type2: Option<Type>,
    /// 포획률(3~255). 클수록 잡기 쉽습니다. 메가 폼은 원래 종과 같은 값입니다.
    /// 예: 마기아나 3(매우 어려움), 무장조 25, 아머까오 45, 구구 255(매우 쉬움)
    /// u8 은 0~255 범위의 부호 없는 정수라 포획률에 딱 맞습니다.
    pub capture_rate: u8,
}

impl Pokemon {
    /// 이 포켓몬의 타입을 1개 또는 2개짜리 목록으로 돌려줍니다.
    /// `for t in pokemon.types() { … }` 처럼 쓰면 Option 을 신경 쓰지 않아도 됩니다.
    pub fn types(&self) -> Vec<Type> {
        match self.type2 {
            Some(t2) => vec![self.type1, t2],
            None => vec![self.type1],
        }
    }

    /// "리자몽 (불꽃/비행)" 같은 표시용 문자열.
    pub fn label(&self) -> String {
        let types: Vec<&str> = self.types().iter().map(|t| t.name()).collect();
        format!("{} ({})", self.name, types.join("/"))
    }
}

/// 내장된 도감 전체.
///
/// `OnceLock` 은 "딱 한 번만 채워지는 칸"입니다. 처음 부를 때 JSON 을 파싱해서 넣고,
/// 그다음부터는 넣어 둔 값을 그대로 돌려줍니다. 여러 요청이 동시에 불러도 안전합니다.
/// 돌려주는 참조가 `'static` 이라서, 여기서 꺼낸 `&Pokemon` 은 어디에 들고 다녀도
/// 수명 문제가 생기지 않습니다.
pub fn pokedex() -> &'static [Pokemon] {
    static POKEDEX: OnceLock<Vec<Pokemon>> = OnceLock::new();
    POKEDEX.get_or_init(|| {
        // 내장 데이터라 형식이 틀리면 프로그램을 고쳐야 하는 버그입니다. 그래서 panic 으로 둡니다.
        // (`cargo test` 의 `내장_도감을_읽는다` 가 배포 전에 잡아 줍니다)
        serde_json::from_str(POKEDEX_JSON).expect("pokemon.json 형식이 잘못됐습니다")
    })
}

/// 이름으로 포켓몬을 찾습니다. 없으면 `None`.
pub fn find<'a>(pokedex: &'a [Pokemon], name: &str) -> Option<&'a Pokemon> {
    pokedex.iter().find(|p| p.name == name)
}

/// 도감에 없는 이름이 들어왔을 때의 오류. API 에서는 400 응답으로 바꿔서 쓰면 됩니다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownPokemon(pub String);

impl fmt::Display for UnknownPokemon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' 은(는) 도감에 없습니다.", self.0)
    }
}

impl std::error::Error for UnknownPokemon {}

/// 이름 목록을 포켓몬 목록으로 바꿉니다. 하나라도 없으면 그 이름으로 `Err` 를 돌려줍니다.
/// (CLI 시절 main 에 있던 "인자 → 팀" 변환을 옮긴 것입니다)
pub fn find_team<S: AsRef<str>>(names: &[S]) -> Result<Vec<&'static Pokemon>, UnknownPokemon> {
    names
        .iter()
        .map(|name| {
            let name = name.as_ref();
            find(pokedex(), name).ok_or_else(|| UnknownPokemon(name.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 내장_도감을_읽는다() {
        assert!(!pokedex().is_empty());
    }

    #[test]
    fn id는_겹치지_않는다() {
        let mut ids: Vec<u32> = pokedex().iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), pokedex().len());
    }

    #[test]
    fn 메가_폼은_원래_종과_species_id가_같다() {
        let team = find_team(&["리자몽", "메가리자몽X"]).unwrap();
        assert_ne!(team[0].id, team[1].id);
        assert_eq!(team[0].species_id, team[1].species_id);
    }

    #[test]
    fn 이름으로_팀을_만든다() {
        let team = find_team(&["리자몽", "갸라도스"]).unwrap();
        assert_eq!(team.len(), 2);
        assert_eq!(team[0].name, "리자몽");
    }

    #[test]
    fn 없는_이름이면_그_이름을_알려준다() {
        let err = find_team(&["리자몽", "없는몬"]).unwrap_err();
        assert_eq!(err, UnknownPokemon("없는몬".to_string()));
    }
}
