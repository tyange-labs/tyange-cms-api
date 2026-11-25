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

// [6단계 — 직접 작성] 빈자리 추천
// pokedex 에서 team 에 없는 포켓몬을 한 마리씩 넣어 보고, 점수가 좋은 3마리를 돌려줍니다.
//
// TODO(C2단계): 3마리만 돌려주지 말고, 후보 "전체"를 근거와 함께 순위대로 돌려주세요.
//
// 왜: 나중에 GUI 에서 추천 목록을 쭉 보여주고 사용자가 고르게 하려고 합니다. 그런데 지금은
//   - `take(3)` 으로 함수 안에서 잘라 버려서, 몇 개를 보여줄지 쓰는 쪽(API·GUI)이 못 정하고
//   - `map(|r| r.2)` 에서 구멍 수·점수를 버려서, 화면에 "넣으면 구멍 2개, 점수 +5" 같은 근거를 못 보여줍니다.
//
// 1. 후보 하나의 결과를 담는 구조체를 이 파일에 만드세요. (예: `Recommendation` — 이름은 자유)
//    - 어떤 필드를 둘지가 이 단계의 핵심입니다. 지금 튜플에 이미 있는 것(후보, 구멍 수, 점수)에서 시작하고,
//      GUI 에 무엇을 보여주고 싶은지 생각해서 더하세요. 힌트: 구멍 "수" 뿐 아니라 "어떤 타입이" 구멍인지는
//      `weak_spot` 이 이미 돌려줍니다(`Vec<Type>`).
//    - 후보는 `&'a Pokemon`(참조)로 들고 있으면 됩니다. 도감이 `'static` 이라 복사(`clone`)할 필요가 없어요.
//      참조를 필드로 가진 구조체는 수명 매개변수가 붙습니다: `struct 이름<'a> { pokemon: &'a Pokemon, … }`
//    - JSON 으로 나가야 하니 위의 `Weakness` 처럼 `#[derive(Debug, Clone, Serialize)]` 를 붙이세요.
//      (`&Pokemon` 필드도 `Pokemon` 이 `Serialize` 라서 그대로 됩니다)
//
// 2. 튜플 대신 그 구조체를 `Vec` 에 쌓고, 정렬 key 도 필드 이름으로 바꾸세요.
//    `k.0`, `k.1` 보다 `k.spot_count` 처럼 이름이 보이면 B단계의 `k.2`/`k.3` 같은 실수가 줄어듭니다.
//    정렬 순서(구멍 수 → 점수 → 포획률)는 그대로 둡니다.
//
// 3. 반환 타입을 `Vec<구조체<'a>>` 로 바꾸고 `take(3)` 을 지우세요.
//
// 4. `recommend_swap` 도 같은 방식으로. 여기선 "뺄 팀원"이 하나 더 있습니다 — 구조체를 따로 만들지,
//    같은 구조체에 `Option` 필드로 넣을지는 직접 골라 보세요(둘 다 괜찮고, 장단점이 있습니다).
//
// 5. 확인: 반환 타입이 바뀌어서 아래 `tests` 의 두 추천 테스트가 컴파일 에러가 납니다
//    (`picks[0]` 이 이제 `&Pokemon` 이 아니니까). 새 구조체에서 포켓몬을 꺼내도록 고치고,
//    테스트 하나를 더 써 보세요 — 예: "결과 개수 == 후보 수(도감 수 - 팀에 있는 수)".
//    그다음 `cargo test -p poketry`.
//
// 덤(선택): 후보에서 팀원을 빼는 비교가 지금은 `t.name == p.name` 입니다. 이름은 표시용이라
//   `id` 로 비교하는 게 더 안전합니다(CLAUDE.md "이름이 아니라 id로 잇는다").
//
// 이 단계가 끝나면 cms-api `src/poketry_api.rs` 의 가이드 ④(결과 타입 설명)가 낡게 됩니다 — 말해 주면 맞춰 고쳐 둡니다.
pub fn recommend<'a>(team: &[&Pokemon], pokedex: &'a [Pokemon]) -> Vec<&'a Pokemon> {
    let mut result: Vec<(usize, i32, &Pokemon)> = Vec::new();

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
        result.push((spot_count, score, candidate));
    }

    // 정렬: 구멍 수 적은 순 → 점수 높은 순 → 포획률 높은 순(잡기 쉬운 순). (B단계)
    // u8 인 포획률은 `-` 를 못 붙여서 `Reverse` 로 뒤집습니다.
    result.sort_by_key(|k| (k.0, -k.1, Reverse(k.2.capture_rate)));
    result.into_iter().map(|r| r.2).take(3).collect()
}

/// 6마리 팀에서 (뺄 팀원, 넣을 후보) 쌍을 점수순으로 3개 돌려줍니다.
pub fn recommend_swap<'a>(
    team: &[&'a Pokemon],
    pokedex: &'a [Pokemon],
) -> Vec<(&'a Pokemon, &'a Pokemon)> {
    let mut result: Vec<(usize, i32, &Pokemon, &Pokemon)> = Vec::new();

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
            result.push((spot_count, score, team[i], candidate));
        }
    }

    // 정렬은 `recommend` 와 같습니다. 포획률은 "넣을 후보"(k.3)의 것 — 뺄 팀원(k.2)이 아니라.
    result.sort_by_key(|k| (k.0, -k.1, Reverse(k.3.capture_rate)));
    result.into_iter().map(|r| (r.2, r.3)).take(3).collect()
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
            spots_with(picks[0]),
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
        assert_eq!(swaps[0].1.name, "아머까오");
    }
}
