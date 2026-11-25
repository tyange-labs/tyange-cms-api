//! 팀 상성 분석. (CLI 시절 main.rs 에서 직접 작성한 함수들을 그대로 옮기고 `pub` 만 붙였습니다)
//!
//! 이 모듈의 함수들은 화면 출력(`println!`)을 하지 않습니다. 계산 결과를 값으로 돌려주고,
//! 그 값을 JSON 으로 내보내는 일은 cms-api 의 라우트(`src/poketry_api.rs`)가 맡습니다.

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
        // TODO(C단계 — 버그 점검): 이 줄을 다시 보세요.
        //
        // `weaknesses` 는 공격 타입 18개마다 한 줄씩 들어 있는 표라서 `.len()` 은 언제나 18입니다.
        // 그래서 지금은 정렬 1순위(구멍 수)가 모든 후보에게 똑같아서 사실상 점수로만 정렬됩니다.
        // 아래 `recommend_swap` 에서는 같은 자리에 무엇을 썼는지 비교해 보세요.
        //
        // 예: 5마리 예시(리자몽 가디안 루카리오 갸라도스 마기라스)에는 넣으면 구멍이 2개가 되는
        //     후보가 있는데, 지금 추천 1~3등(입치트·메가입치트·클레피)은 전부 구멍 3개짜리입니다.
        //
        // 고친 뒤 확인: 맨 아래 `tests` 모듈의 `recommend_구멍_수가_정렬_1순위다` 테스트에서
        // `#[ignore]` 줄을 지우고 `cargo test -p poketry` 를 돌리면 통과해야 합니다.
        let spot_count = weaknesses.len();
        result.push((spot_count, score, candidate));
    }

    // TODO(B단계): 정렬 3순위로 포획률을 추가하세요. 목표 key: (구멍 수, -점수, -포획률)
    //
    // 1. 포획률은 후보에서 꺼냅니다: `candidate.capture_rate` (타입은 u8)
    //    튜플에 따로 넣을 필요는 없어요. 튜플의 후보(k.2)에서 바로 꺼낼 수 있습니다.
    //
    // 2. "큰 순"으로 하려면 부호를 뒤집어야 하는데, u8 은 음수가 될 수 없어서
    //    `-k.2.capture_rate` 는 컴파일 에러입니다. 방법은 둘 중 하나:
    //    - 부호 있는 정수로 바꾼 뒤 뒤집기: `-(값 as i32)`   (`as` 는 타입 변환)
    //    - 뒤집기 전용 감싸개 쓰기: `std::cmp::Reverse(값)`  (Reverse 로 감싸면 큰 값이 앞에 옵니다)
    //
    // 3. 확인: CLI 가 없어졌으니 테스트로 확인합니다. 맨 아래 `tests` 모듈의
    //    `recommend_swap_포획률이_정렬_3순위다` 테스트의 `#[ignore]` 를 지우고 돌려 보세요.
    result.sort_by_key(|k| (k.0, -k.1));
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

    // TODO(B단계): recommend 와 같은 방식으로 포획률을 정렬 3순위에 추가하세요.
    //
    // - 여기서는 튜플이 (구멍 수, 점수, 뺄 팀원, 넣을 후보) 라서
    //   포획률은 "넣을 후보"(k.3)의 것을 씁니다. 뺄 팀원(k.2)의 포획률이 아니에요!
    //
    // - 확인: 6마리 예시(… 님피아)로 돌리면 지금은 무장조(25) → 메가무장조(25) → 아머까오(45)
    //   순서인데, 포획률이 들어가면 아머까오(45)가 1등으로 올라와야 합니다.
    //   이 확인을 테스트로 옮겨 두었습니다: `recommend_swap_포획률이_정렬_3순위다`
    result.sort_by_key(|k| (k.0, -k.1));
    result.into_iter().map(|r| (r.2, r.3)).take(3).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pokemon::{find_team, pokedex};

    fn mon(type1: Type, type2: Option<Type>) -> Pokemon {
        Pokemon {
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

    // ── 아래 두 테스트는 B·C단계를 끝내면 `#[ignore]` 를 지워서 켭니다 ──
    // `#[ignore]` 가 붙은 테스트는 `cargo test` 에서 건너뜁니다(결과에 "ignored" 로 나옵니다).
    // 지금 상태로 결과만 보고 싶으면: `cargo test -p poketry -- --ignored`

    /// C단계 확인용. 1등 추천을 넣은 팀의 "보완할 타입" 수가, 모든 후보 중 가장 적어야 합니다.
    /// (지금은 이 예시에서 구멍 2개짜리 후보가 있는데도 구멍 3개짜리가 1등으로 나옵니다)
    #[test]
    #[ignore = "C단계(recommend 구멍 수)를 고친 뒤에 켭니다"]
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

        assert_eq!(spots_with(picks[0]), best_possible, "구멍 수가 가장 적은 후보가 1등이어야 합니다");
    }

    /// B단계 확인용. CLI 시절 6마리 예시를 그대로 옮겼습니다.
    /// 지금은 무장조(25) → 메가무장조(25) → 아머까오(45) 순서라 실패합니다.
    #[test]
    #[ignore = "B단계(포획률 정렬)를 끝낸 뒤에 켭니다"]
    fn recommend_swap_포획률이_정렬_3순위다() {
        let team = find_team(&["리자몽", "가디안", "루카리오", "갸라도스", "마기라스", "님피아"])
            .unwrap();
        let swaps = recommend_swap(&team, pokedex());
        assert_eq!(swaps[0].1.name, "아머까오");
    }
}
