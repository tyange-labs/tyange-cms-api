//! 포켓몬 구조체와 데이터 파일 읽기. (Claude가 만든 뼈대)

use crate::types::Type;
use serde::Deserialize;
use std::error::Error;
use std::fs;

/// 포켓몬 한 마리. 단일 타입이면 `type2` 가 `None` 입니다.
#[derive(Debug, Clone, Deserialize)]
pub struct Pokemon {
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

/// JSON 파일에서 포켓몬 목록을 읽습니다.
pub fn load_pokedex(path: &str) -> Result<Vec<Pokemon>, Box<dyn Error>> {
    let text = fs::read_to_string(path).map_err(|e| format!("{path} 파일을 읽지 못했습니다: {e}"))?;
    let list: Vec<Pokemon> =
        serde_json::from_str(&text).map_err(|e| format!("{path} 형식이 잘못됐습니다: {e}"))?;
    Ok(list)
}

/// 이름으로 포켓몬을 찾습니다. 없으면 `None`.
pub fn find<'a>(pokedex: &'a [Pokemon], name: &str) -> Option<&'a Pokemon> {
    pokedex.iter().find(|p| p.name == name)
}
