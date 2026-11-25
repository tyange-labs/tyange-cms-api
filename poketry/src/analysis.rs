//! 팀 상성 분석. (CLI 시절 main.rs 에서 직접 작성한 함수들을 그대로 옮기고 `pub` 만 붙였습니다)
//!
//! 이 모듈의 함수들은 화면 출력(`println!`)을 하지 않습니다. 계산 결과를 값으로 돌려주고,
//! 그 값을 JSON 으로 내보내는 일은 cms-api 의 라우트(`src/poketry_api.rs`)가 맡습니다.

use std::cmp::Reverse;

use serde::Serialize;

use crate::pokemon::Pokemon;
use crate::types::{Type, effectiveness};

/// [3단계 — 직접 작성]
/// `attack` 타입의 기술을 `defender` 가 맞았을 때의 배율.
///
/// 복합 타입이면 두 타입 각각의 배율을 곱합니다. (그래서 4.0 과 0.25 가 나옵니다)
/// 재료: `effectiveness(attack, 방어타입)` 과 `defender.types()`.
pub fn defense_multiplier(attack: Type, defender: &Pokemon) -> f32 {
    let first_multiplier = effectiveness(attack, defender.type1);

    if let Some(second_type) = defender.type2 {
        let second_multiplier = effectiveness(attack, second_type);

        return first_multiplier * second_multiplier;
    }

    first_multiplier
}

/// 공격 타입 하나에 대한 팀의 약점/반감 마릿수.
///
/// `Serialize` 를 붙여 두어서 `serde_json` 으로 바로 JSON 이 됩니다.
/// (단, cms-api 의 poem-openapi 응답에는 `Object` 가 필요해서 그쪽에서 따로 옮겨 담습니다.
///  자세한 이유는 cms-api `src/poketry_api.rs` 의 가이드를 보세요.)
#[derive(Debug, Clone, Serialize)]
pub struct Weakness {
    pub attack_type: Type,
    pub under_multiplier_count: i32,
    pub twice_multiplier_count: i32,
}

// [5단계 — 직접 작성] 팀 약점표
// 공격 타입마다 "약점(2배 이상)인 마릿수"와 "반감·무효(1배 미만)인 마릿수"를 셉니다.
pub fn team_weakness(team: &[&Pokemon]) -> Vec<Weakness> {
    let mut result: Vec<Weakness> = Vec::new();

    for attack in Type::ALL {
        // 바깥: 공격 타입 18개
        let mut weak = 0; // 공격 타입이 바뀔 때마다 0부터
        let mut resist = 0;

        for p in team {
            // 안쪽: 팀원 한 마리씩
            let m = defense_multiplier(attack, p);
            if m >= 2.0 {
                weak += 1;
            } else if m < 1.0 {
                resist += 1;
            }
        }

        result.push(Weakness {
            // 팀원을 다 센 "뒤"에 한 줄 추가
            attack_type: attack,
            twice_multiplier_count: weak,
            under_multiplier_count: resist,
        });
    }

    result
}

/// 보완할 타입: 약점인 마릿수가 받아 주는 마릿수보다 많은 공격 타입들.
pub fn weak_spot(weaknesses: &[Weakness]) -> Vec<Type> {
    let mut result: Vec<Type> = Vec::new();

    for weakness in weaknesses {
        if weakness.twice_multiplier_count > weakness.under_multiplier_count {
            result.push(weakness.attack_type);
        }
    }

    result
}

/// 팀 점수: 공격 타입마다 (받아 주는 마릿수 - 약점 마릿수) 를 더한 값. 클수록 좋습니다.
pub fn team_score(weakness: &[Weakness]) -> i32 {
    let mut result = 0;

    for weak in weakness {
        result = result + (weak.under_multiplier_count - weak.twice_multiplier_count);
    }

    result
}

/// 빈자리 추천 결과 하나: 이 후보를 넣었을 때의 구멍 수·점수. (C2단계)
///
/// 화면에 근거("넣으면 구멍 2개, 점수 +5")를 보여줄 수 있게 숫자를 함께 돌려줍니다.
#[derive(Debug, Clone, Serialize)]
pub struct Recommendation<'a> {
    pub spot_count: usize,
    pub score: i32,
    pub candidate: &'a Pokemon,
}

// [6단계 — 직접 작성] 빈자리 추천
// pokedex 에서 team 에 없는 포켓몬을 한 마리씩 넣어 보고, 후보 "전체"를 좋은 순서대로 돌려줍니다.
// 몇 개를 보여줄지는 쓰는 쪽(API·GUI)이 정합니다.
pub fn recommend<'a>(team: &[&Pokemon], pokedex: &'a [Pokemon]) -> Vec<Recommendation<'a>> {
    let mut result: Vec<Recommendation> = Vec::new();

    let candidates: Vec<&Pokemon> = pokedex
        .iter()
        .filter(|p| !team.iter().any(|t| t.name == p.name))
        .collect();

    for candidate in candidates {
        let mut new_team = team.to_vec();
        new_team.push(candidate);
        let weaknesses = team_weakness(&new_team);
        let score = team_score(&weaknesses);
        // (C단계) `weaknesses.len()` 은 공격 타입 수라 언제나 18 — 구멍 수는 `weak_spot` 으로 셉니다.
        let spot_count = weak_spot(&weaknesses).len();
        result.push(Recommendation {
            spot_count,
            score,
            candidate,
        });
    }

    // 정렬: 구멍 수 적은 순 → 점수 높은 순 → 포획률 높은 순(잡기 쉬운 순). (B단계)
    // u8 인 포획률은 `-` 를 못 붙여서 `Reverse` 로 뒤집습니다.
    result.sort_by_key(|k| (k.spot_count, -k.score, Reverse(k.candidate.capture_rate)));
    result
}

/// 교체 추천 결과 하나: `out` 을 빼고 `candidate` 를 넣었을 때의 구멍 수·점수.
#[derive(Debug, Clone, Serialize)]
pub struct Swap<'a> {
    pub out: &'a Pokemon,
    pub candidate: &'a Pokemon,
    pub spot_count: usize,
    pub score: i32,
}

/// 6마리 팀에서 (뺄 팀원, 넣을 후보) 쌍 "전체"를 좋은 순서대로 돌려줍니다. 정렬은 `recommend` 와 같습니다.
pub fn recommend_swap<'a>(team: &[&'a Pokemon], pokedex: &'a [Pokemon]) -> Vec<Swap<'a>> {
    let mut result: Vec<Swap> = Vec::new();

    let candidates: Vec<&Pokemon> = pokedex
        .iter()
        .filter(|p| !team.iter().any(|t| t.name == p.name))
        .collect();

    for i in 0..team.len() {
        for candidate in &candidates {
            let mut new_team = team.to_vec();
            new_team[i] = candidate;
            let weaknesses = team_weakness(&new_team);
            let score = team_score(&weaknesses);
            let spot_count = weak_spot(&weaknesses).len();
            result.push(Swap {
                out: team[i],
                candidate,
                spot_count,
                score,
            });
        }
    }

    // 정렬은 `recommend` 와 같습니다. 포획률은 "넣을 후보"(k.3)의 것 — 뺄 팀원(k.2)이 아니라.
    result.sort_by_key(|k| (k.spot_count, -k.score, Reverse(k.candidate.capture_rate)));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pokemon::{find_team, pokedex};

    fn mon(type1: Type, type2: Option<Type>) -> Pokemon {
        Pokemon {
            id: 0,         // 상성 테스트에는 쓰이지 않는 값
            species_id: 0, // 〃
            name: "테스트".to_string(),
            type1,
            type2,
            capture_rate: 45, // 상성 테스트에는 쓰이지 않는 값
        }
    }

    // 3단계의 목표: `cargo test` 에서 아래 세 테스트가 통과하면 됩니다.

    #[test]
    fn 단일_타입은_상성표_그대로() {
        let p = mon(Type::Grass, None);
        assert_eq!(defense_multiplier(Type::Fire, &p), 2.0);
        assert_eq!(defense_multiplier(Type::Water, &p), 0.5);
    }

    #[test]
    fn 복합_타입은_곱한다() {
        let p = mon(Type::Grass, Some(Type::Flying));
        assert_eq!(defense_multiplier(Type::Ice, &p), 4.0);
        assert_eq!(defense_multiplier(Type::Grass, &p), 0.25);
    }

    #[test]
    fn 무효가_하나라도_있으면_0() {
        let p = mon(Type::Water, Some(Type::Flying));
        assert_eq!(defense_multiplier(Type::Ground, &p), 0.0);
    }

    // ── B·C단계 확인 테스트 ──

    /// C단계 확인용. 1등 추천을 넣은 팀의 "보완할 타입" 수가, 모든 후보 중 가장 적어야 합니다.
    /// (고치기 전에는 이 예시에서 구멍 2개짜리 후보가 있는데도 구멍 3개짜리가 1등으로 나왔습니다)
    #[test]
    fn recommend_구멍_수가_정렬_1순위다() {
        let team = find_team(&["리자몽", "가디안", "루카리오", "갸라도스", "마기라스"]).unwrap();
        let spots_with = |p: &Pokemon| {
            let mut t = team.clone();
            t.push(p);
            weak_spot(&team_weakness(&t)).len()
        };

        let best_possible = pokedex()
            .iter()
            .filter(|c| !team.iter().any(|t| t.name == c.name))
            .map(|c| spots_with(c))
            .min()
            .unwrap();
        let picks = recommend(&team, pokedex());

        assert_eq!(
            spots_with(picks[0].candidate),
            best_possible,
            "구멍 수가 가장 적은 후보가 1등이어야 합니다"
        );
    }

    /// B단계 확인용. CLI 시절 6마리 예시를 그대로 옮겼습니다.
    /// 포획률이 없으면 무장조(25) → 메가무장조(25) → 아머까오(45) 순서가 됩니다(구멍 수·점수가 같아서).
    #[test]
    fn recommend_swap_포획률이_정렬_3순위다() {
        let team = find_team(&[
            "리자몽",
            "가디안",
            "루카리오",
            "갸라도스",
            "마기라스",
            "님피아",
        ])
        .unwrap();
        let swaps = recommend_swap(&team, pokedex());
        assert_eq!(swaps[0].candidate.name, "아머까오");
    }
}
