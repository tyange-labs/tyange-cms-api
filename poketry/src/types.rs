use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Type {
    #[serde(rename = "노말")]
    Normal,
    #[serde(rename = "불꽃")]
    Fire,
    #[serde(rename = "물")]
    Water,
    #[serde(rename = "전기")]
    Electric,
    #[serde(rename = "풀")]
    Grass,
    #[serde(rename = "얼음")]
    Ice,
    #[serde(rename = "격투")]
    Fighting,
    #[serde(rename = "독")]
    Poison,
    #[serde(rename = "땅")]
    Ground,
    #[serde(rename = "비행")]
    Flying,
    #[serde(rename = "에스퍼")]
    Psychic,
    #[serde(rename = "벌레")]
    Bug,
    #[serde(rename = "바위")]
    Rock,
    #[serde(rename = "고스트")]
    Ghost,
    #[serde(rename = "드래곤")]
    Dragon,
    #[serde(rename = "악")]
    Dark,
    #[serde(rename = "강철")]
    Steel,
    #[serde(rename = "페어리")]
    Fairy,
}

use Type::*;

impl Type {
    pub const ALL: [Type; 18] = [
        Normal, Fire, Water, Electric, Grass, Ice, Fighting, Poison, Ground, Flying, Psychic, Bug,
        Rock, Ghost, Dragon, Dark, Steel, Fairy,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Normal => "노말",
            Fire => "불꽃",
            Water => "물",
            Electric => "전기",
            Grass => "풀",
            Ice => "얼음",
            Fighting => "격투",
            Poison => "독",
            Ground => "땅",
            Flying => "비행",
            Psychic => "에스퍼",
            Bug => "벌레",
            Rock => "바위",
            Ghost => "고스트",
            Dragon => "드래곤",
            Dark => "악",
            Steel => "강철",
            Fairy => "페어리",
        }
    }
}

pub fn effectiveness(attack: Type, defend: Type) -> f32 {
    match (attack, defend) {
        (Normal, Ghost)
        | (Electric, Ground)
        | (Fighting, Ghost)
        | (Poison, Steel)
        | (Ground, Flying)
        | (Psychic, Dark)
        | (Ghost, Normal)
        | (Dragon, Fairy) => 0.0,

        (Fire, Grass | Ice | Bug | Steel)
        | (Water, Fire | Ground | Rock)
        | (Electric, Water | Flying)
        | (Grass, Water | Ground | Rock)
        | (Ice, Grass | Ground | Flying | Dragon)
        | (Fighting, Normal | Ice | Rock | Dark | Steel)
        | (Poison, Grass | Fairy)
        | (Ground, Fire | Electric | Poison | Rock | Steel)
        | (Flying, Grass | Fighting | Bug)
        | (Psychic, Fighting | Poison)
        | (Bug, Grass | Psychic | Dark)
        | (Rock, Fire | Ice | Flying | Bug)
        | (Ghost, Psychic | Ghost)
        | (Dragon, Dragon)
        | (Dark, Psychic | Ghost)
        | (Steel, Ice | Rock | Fairy)
        | (Fairy, Fighting | Dragon | Dark) => 2.0,

        (Normal, Rock | Steel)
        | (Fire, Fire | Water | Rock | Dragon)
        | (Water, Water | Grass | Dragon)
        | (Electric, Electric | Grass | Dragon)
        | (Grass, Fire | Grass | Poison | Flying | Bug | Dragon | Steel)
        | (Ice, Fire | Water | Ice | Steel)
        | (Fighting, Poison | Flying | Psychic | Bug | Fairy)
        | (Poison, Poison | Ground | Rock | Ghost)
        | (Ground, Grass | Bug)
        | (Flying, Electric | Rock | Steel)
        | (Psychic, Psychic | Steel)
        | (Bug, Fire | Fighting | Poison | Flying | Ghost | Steel | Fairy)
        | (Rock, Fighting | Ground | Steel)
        | (Ghost, Dark)
        | (Dragon, Steel)
        | (Dark, Fighting | Dark | Fairy)
        | (Steel, Fire | Water | Electric | Steel)
        | (Fairy, Fire | Poison | Steel) => 0.5,

        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 대표적인_상성() {
        assert_eq!(effectiveness(Fire, Grass), 2.0);
        assert_eq!(effectiveness(Water, Water), 0.5);
        assert_eq!(effectiveness(Electric, Ground), 0.0);
        assert_eq!(effectiveness(Normal, Normal), 1.0);
        assert_eq!(effectiveness(Fairy, Dragon), 2.0);
        assert_eq!(effectiveness(Dragon, Fairy), 0.0);
    }

    #[test]
    fn 상성표_칸_수_검산() {
        let count = |m: f32| {
            Type::ALL
                .iter()
                .flat_map(|&a| Type::ALL.iter().map(move |&d| effectiveness(a, d)))
                .filter(|&x| x == m)
                .count()
        };
        assert_eq!(count(2.0), 51);
        assert_eq!(count(0.5), 61);
        assert_eq!(count(0.0), 8);
        assert_eq!(count(1.0), 18 * 18 - 51 - 61 - 8);
    }
}
