//! 타입 18개와 상성표. (Claude가 만든 뼈대 — 고칠 일은 거의 없습니다)

use serde::Deserialize;

/// 포켓몬 타입. JSON에서는 한국어 이름("불꽃", "물" …)으로 적습니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
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
    /// 모든 타입. `for t in Type::ALL { … }` 로 순회할 때 씁니다.
    pub const ALL: [Type; 18] = [
        Normal, Fire, Water, Electric, Grass, Ice, Fighting, Poison, Ground, Flying, Psychic, Bug,
        Rock, Ghost, Dragon, Dark, Steel, Fairy,
    ];

    /// 화면에 출력할 한국어 이름.
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

/// 공격 타입 하나가 방어 타입 "하나"에 주는 배율: 0.0, 0.5, 1.0, 2.0 중 하나.
///
/// 복합 타입 포켓몬에 대한 배율은 여기서 계산하지 않습니다.
/// 그건 main.rs 의 `defense_multiplier` 에서 직접 만듭니다.
pub fn effectiveness(attack: Type, defend: Type) -> f32 {
    match (attack, defend) {
        // 효과가 없다 (0배)
        (Normal, Ghost)
        | (Electric, Ground)
        | (Fighting, Ghost)
        | (Poison, Steel)
        | (Ground, Flying)
        | (Psychic, Dark)
        | (Ghost, Normal)
        | (Dragon, Fairy) => 0.0,

        // 효과가 굉장하다 (2배)
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

        // 효과가 별로다 (0.5배)
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

    /// 상성표 전체에서 2배·0.5배·0배인 칸의 수는 정해져 있습니다(51·61·8).
    /// 표를 옮겨 적다 생긴 오타를 잡기 위한 검산입니다.
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
