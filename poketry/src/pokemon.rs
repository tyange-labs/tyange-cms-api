use crate::types::Type;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::OnceLock;

const POKEDEX_JSON: &str = include_str!("../pokemon.json");

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Pokemon {
    pub id: u32,
    pub species_id: u32,
    pub name: String,
    pub type1: Type,
    #[serde(default)]
    pub type2: Option<Type>,
    pub capture_rate: u8,
}

impl Pokemon {
    pub fn types(&self) -> Vec<Type> {
        match self.type2 {
            Some(t2) => vec![self.type1, t2],
            None => vec![self.type1],
        }
    }

    pub fn label(&self) -> String {
        let types: Vec<&str> = self.types().iter().map(|t| t.name()).collect();
        format!("{} ({})", self.name, types.join("/"))
    }
}

pub fn pokedex() -> &'static [Pokemon] {
    static POKEDEX: OnceLock<Vec<Pokemon>> = OnceLock::new();
    POKEDEX.get_or_init(|| {
        serde_json::from_str(POKEDEX_JSON).expect("pokemon.json 형식이 잘못됐습니다")
    })
}

pub fn find<'a>(pokedex: &'a [Pokemon], name: &str) -> Option<&'a Pokemon> {
    pokedex.iter().find(|p| p.name == name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownPokemon(pub String);

impl fmt::Display for UnknownPokemon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' 은(는) 도감에 없습니다.", self.0)
    }
}

impl std::error::Error for UnknownPokemon {}

pub fn find_team<S: AsRef<str>>(names: &[S]) -> Result<Vec<&'static Pokemon>, UnknownPokemon> {
    names
        .iter()
        .map(|name| {
            let name = name.as_ref();
            find(pokedex(), name).ok_or_else(|| UnknownPokemon(name.to_string()))
        })
        .collect()
}

pub fn find_by_id(pokedex: &[Pokemon], id: u32) -> Option<&Pokemon> {
    pokedex.iter().find(|p| p.id == id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownPokemonId(pub u32);

impl fmt::Display for UnknownPokemonId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "id {} 은(는) 도감에 없습니다.", self.0)
    }
}

impl std::error::Error for UnknownPokemonId {}

pub fn find_team_by_ids(ids: &[u32]) -> Result<Vec<&'static Pokemon>, UnknownPokemonId> {
    ids.iter()
        .map(|&id| find_by_id(pokedex(), id).ok_or(UnknownPokemonId(id)))
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
    fn id로_팀을_만든다() {
        let team = find_team_by_ids(&[6, 10034]).unwrap();
        assert_eq!(team[0].name, "리자몽");
        assert_eq!(team[1].species_id, 6);
    }

    #[test]
    fn 없는_id면_그_id를_알려준다() {
        let err = find_team_by_ids(&[6, 999999]).unwrap_err();
        assert_eq!(err, UnknownPokemonId(999999));
        assert_eq!(err.to_string(), "id 999999 은(는) 도감에 없습니다.");
    }

    #[test]
    fn 없는_이름이면_그_이름을_알려준다() {
        let err = find_team(&["리자몽", "없는몬"]).unwrap_err();
        assert_eq!(err, UnknownPokemon("없는몬".to_string()));
    }
}
