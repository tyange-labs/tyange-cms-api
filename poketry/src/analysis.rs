use std::cmp::Reverse;

use serde::Serialize;

use crate::pokemon::Pokemon;
use crate::types::{Type, effectiveness};

pub fn defense_multiplier(attack: Type, defender: &Pokemon) -> f32 {
    let first_multiplier = effectiveness(attack, defender.type1);

    if let Some(second_type) = defender.type2 {
        let second_multiplier = effectiveness(attack, second_type);

        return first_multiplier * second_multiplier;
    }

    first_multiplier
}

#[derive(Debug, Clone, Serialize)]
pub struct Weakness {
    pub attack_type: Type,
    pub under_multiplier_count: i32,
    pub twice_multiplier_count: i32,
}

pub fn team_weakness(team: &[&Pokemon]) -> Vec<Weakness> {
    let mut result: Vec<Weakness> = Vec::new();

    for attack in Type::ALL {
        let mut weak = 0;
        let mut resist = 0;

        for p in team {
            let m = defense_multiplier(attack, p);
            if m >= 2.0 {
                weak += 1;
            } else if m < 1.0 {
                resist += 1;
            }
        }

        result.push(Weakness {
            attack_type: attack,
            twice_multiplier_count: weak,
            under_multiplier_count: resist,
        });
    }

    result
}

pub fn weak_spot(weaknesses: &[Weakness]) -> Vec<Type> {
    let mut result: Vec<Type> = Vec::new();

    for weakness in weaknesses {
        if weakness.twice_multiplier_count > weakness.under_multiplier_count {
            result.push(weakness.attack_type);
        }
    }

    result
}

pub fn team_score(weakness: &[Weakness]) -> i32 {
    let mut result = 0;

    for weak in weakness {
        result = result + (weak.under_multiplier_count - weak.twice_multiplier_count);
    }

    result
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation<'a> {
    pub spot_count: usize,
    pub score: i32,
    pub candidate: &'a Pokemon,
}

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
        let spot_count = weak_spot(&weaknesses).len();
        result.push(Recommendation {
            spot_count,
            score,
            candidate,
        });
    }

    result.sort_by_key(|k| (k.spot_count, -k.score, Reverse(k.candidate.capture_rate)));
    result
}

#[derive(Debug, Clone, Serialize)]
pub struct Swap<'a> {
    pub out: &'a Pokemon,
    pub candidate: &'a Pokemon,
    pub spot_count: usize,
    pub score: i32,
}

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

    result.sort_by_key(|k| (k.spot_count, -k.score, Reverse(k.candidate.capture_rate)));
    result
}

#[derive(Debug, Clone, Serialize)]
pub struct SlotPick<'a> {
    pub slot_type: Type,
    pub picks: Vec<Recommendation<'a>>,
}

fn type_only(t: Type) -> Pokemon {
    Pokemon {
        id: 0,
        species_id: 0,
        name: t.name().to_string(),
        type1: t,
        type2: None,
        capture_rate: 0,
    }
}

pub fn recommend_slots<'a>(team: &[&Pokemon], pokedex: &'a [Pokemon]) -> Vec<SlotPick<'a>> {
    // TODO(C4단계 ①): 빈자리마다 어울리는 타입을 하나씩 고르고, 타입마다 그 타입을 가진 포켓몬을 추천하세요.
    //
    // 1. 빈자리 수: `6 - team.len()`. 6마리 이상이면 빈 Vec 을 돌려주세요(빼기가 음수가 되면 usize 라 패닉).
    //    `6usize.saturating_sub(team.len())` 를 쓰면 0 에서 멈춥니다.
    //
    // 2. 타입 고르기(욕심쟁이 방식): 빈자리 수만큼 반복합니다.
    //    - 지금까지 고른 타입을 `chosen: Vec<Type>` 에 모읍니다.
    //    - 매 반복마다 `Type::ALL` 중 아직 안 고른 타입 `t` 를 하나씩 넣어 봅니다.
    //      "그 타입 하나만 가진 가상의 포켓몬" 은 `type_only(t)` 가 만들어 줍니다.
    //      시험용 팀 = `team` + 지금까지 고른 타입들의 가상 포켓몬 + `type_only(t)`.
    //      (가상 포켓몬은 `Vec<Pokemon>` 에 값으로 만들어 두고, 시험용 팀 `Vec<&Pokemon>` 에는 참조를 넣으세요)
    //    - 시험용 팀의 `team_weakness` → `weak_spot(..).len()` 과 `team_score(..)` 를 구해서
    //      `recommend` 와 같은 기준 `(구멍 수, -점수)` 가 가장 작은 `t` 를 고릅니다.
    //      `min_by_key` 를 쓰면 같은 값일 때 앞쪽(`Type::ALL` 순서)이 뽑혀서 결과가 매번 같습니다.
    //    - 고른 타입을 `chosen` 에 넣고 다음 자리로.
    //
    // 3. 타입마다 추천: `recommend(team, pokedex)` 를 **한 번만** 불러 두고(원래 팀 기준 순위),
    //    고른 타입마다 `candidate.types()` 에 그 타입이 들어 있는 것만 걸러 `SlotPick { slot_type, picks }` 로 담습니다.
    //    `Recommendation` 은 `Clone` 이라 걸러 낸 걸 `.cloned()` 로 복사할 수 있어요.
    //
    // 확인: 아래 tests 의 `빈자리_…`·`팀이_다_차면_…` 두 테스트에서 `#[ignore]` 를 지우고 `cargo test -p poketry`.
    let _ = (team, pokedex, type_only);
    todo!()
}

#[derive(Debug, Clone, Serialize)]
pub struct WeakSpotFix<'a> {
    pub weak_type: Type,
    pub swaps: Vec<Swap<'a>>,
}

pub fn recommend_fixes<'a>(team: &[&'a Pokemon], pokedex: &'a [Pokemon]) -> Vec<WeakSpotFix<'a>> {
    // TODO(C4단계 ②): 약점 타입마다, 그 타입을 견디는 포켓몬으로 바꾸는 교체 후보를 모으세요.
    //
    // 1. 약점 타입: `weak_spot(&team_weakness(team))`.
    // 2. 교체 후보 전체: `recommend_swap(team, pokedex)` 를 **한 번만** 부릅니다(이미 좋은 순서로 정렬돼 있어요).
    // 3. 약점 타입 `w` 마다: 교체 후보 중 들어오는 포켓몬(`candidate`)이 `w` 를 견디는 것만 남깁니다.
    //    "견딘다" = `defense_multiplier(w, candidate) < 1.0` (반감·무효).
    //    `WeakSpotFix { weak_type: w, swaps }` 로 담으세요. 순서는 `weak_spot` 이 준 순서 그대로.
    //
    // 생각해 볼 거리 (선택): 빠지는 포켓몬(`out`)이 원래 `w` 를 견디던 녀석이면 바꿔 봐야 그 약점은 안 줄어듭니다.
    // 그런 교체도 걸러 낼지 정해 보세요.
    //
    // 확인: 아래 tests 의 `약점_타입마다_…` 테스트에서 `#[ignore]` 를 지우고 `cargo test -p poketry`.
    let _ = (team, pokedex);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pokemon::{find_team, pokedex};

    fn mon(type1: Type, type2: Option<Type>) -> Pokemon {
        Pokemon {
            id: 0,
            species_id: 0,
            name: "테스트".to_string(),
            type1,
            type2,
            capture_rate: 45,
        }
    }

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

    #[test]
    #[ignore]
    fn 빈자리_수만큼_서로_다른_타입을_고르고_그_타입_포켓몬만_추천한다() {
        let team = find_team(&["리자몽", "가디안", "루카리오"]).unwrap();
        let slots = recommend_slots(&team, pokedex());

        assert_eq!(slots.len(), 3);
        for (i, slot) in slots.iter().enumerate() {
            assert!(!slot.picks.is_empty(), "{} 타입 추천이 비었습니다", slot.slot_type.name());
            assert!(
                slots[..i].iter().all(|s| s.slot_type != slot.slot_type),
                "같은 타입을 두 번 골랐습니다"
            );
            for pick in &slot.picks {
                assert!(pick.candidate.types().contains(&slot.slot_type));
                assert!(!team.iter().any(|t| t.id == pick.candidate.id));
            }
        }
    }

    #[test]
    #[ignore]
    fn 팀이_다_차면_빈자리_추천은_없다() {
        let team = find_team(&["리자몽", "가디안", "루카리오", "갸라도스", "마기라스", "님피아"])
            .unwrap();
        assert!(recommend_slots(&team, pokedex()).is_empty());
    }

    #[test]
    #[ignore]
    fn 약점_타입마다_그_타입을_견디는_교체만_추천한다() {
        let team = find_team(&["리자몽", "가디안", "루카리오", "갸라도스", "마기라스", "님피아"])
            .unwrap();
        let spots = weak_spot(&team_weakness(&team));
        let fixes = recommend_fixes(&team, pokedex());

        assert!(!spots.is_empty(), "테스트 팀에 약점이 있어야 의미가 있습니다");
        assert_eq!(fixes.iter().map(|f| f.weak_type).collect::<Vec<_>>(), spots);
        for fix in &fixes {
            assert!(!fix.swaps.is_empty());
            for s in &fix.swaps {
                assert!(defense_multiplier(fix.weak_type, s.candidate) < 1.0);
            }
        }
    }
}
