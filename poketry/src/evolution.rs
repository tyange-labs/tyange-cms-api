use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const EVOLUTIONS_JSON: &str = include_str!("../evolutions.json");

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
