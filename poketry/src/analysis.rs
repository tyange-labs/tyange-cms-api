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
    pub slot_types: Vec<Type>,
    pub picks: Vec<Recommendation<'a>>,
}

fn shape_of(p: &Pokemon) -> Vec<Type> {
    let mut types = p.types();
    types.sort_by_key(|&t| t as u8);
    types
}

fn type_shapes<'p>(pokemons: impl IntoIterator<Item = &'p Pokemon>) -> Vec<Vec<Type>> {
    let mut shapes: Vec<Vec<Type>> = Vec::new();
    for p in pokemons {
        let shape = shape_of(p);
        if !shapes.contains(&shape) {
            shapes.push(shape);
        }
    }
    shapes
}

fn team_key(members: &[&Pokemon]) -> (usize, i32) {
    let weaknesses = team_weakness(members);
    (weak_spot(&weaknesses).len(), -team_score(&weaknesses))
}

fn virtual_mon(shape: &[Type]) -> Pokemon {
    Pokemon {
        id: 0,
        species_id: 0,
        name: shape.iter().map(|t| t.name()).collect::<Vec<_>>().join("/"),
        type1: shape[0],
        type2: shape.get(1).copied(),
        capture_rate: 0,
    }
}

pub fn recommend_slots<'a>(team: &[&Pokemon], pokedex: &'a [Pokemon]) -> Vec<SlotPick<'a>> {
    let empty = 6usize.saturating_sub(team.len());
    let shapes = type_shapes(
        pokedex
            .iter()
            .filter(|p| !team.iter().any(|t| t.id == p.id)),
    );

    let mut chosen: Vec<Vec<Type>> = Vec::new();
    let mut chosen_mons: Vec<Pokemon> = Vec::new();

    for _ in 0..empty {
        let mut best: Option<(&Vec<Type>, (usize, i32))> = None;

        for shape in &shapes {
            if chosen.contains(shape) {
                continue;
            }

            let candidate = virtual_mon(shape);
            let mut trial: Vec<&Pokemon> = team.to_vec();
            trial.extend(chosen_mons.iter());
            trial.push(&candidate);

            let key = team_key(&trial);

            let better = match &best {
                None => true,
                Some((_, best_key)) => key < *best_key,
            };
            if better {
                best = Some((shape, key));
            }
        }

        let (shape, _) = best.unwrap();
        chosen.push(shape.clone());
        chosen_mons.push(virtual_mon(shape));
    }

    // TODO(C4단계 ① 마): 다듬기 — 이미 고른 자리를 다른 조합으로 바꿔서 팀이 더 좋아지면 바꾸세요.
    //   왜: 위 욕심쟁이는 한 번 고른 자리를 되돌리지 못해서, 나중 자리를 보고 나면 앞자리를 다르게 골랐어야 했던 경우를 놓칩니다.
    //
    //   해야 할 일:
    //   1. 가장 바깥은 "한 바퀴 돌았을 때 바뀐 게 하나라도 있으면 또 한 바퀴"를 되풀이하는 반복입니다.
    //      바뀐 게 있었는지 기억할 `bool` 변수가 하나 필요하고, 한 바퀴 동안 안 바뀌었으면 반복을 끝냅니다.
    //   2. 한 바퀴 안에서는 고른 자리(`chosen` 의 0번부터 끝까지)를 하나씩 봅니다.
    //   3. 자리 하나를 볼 때, 먼저 지금 팀 전체(`team` + `chosen_mons` 전부)의 `team_key` 를 구해 둡니다.
    //   4. 그 자리에 도감의 모든 조합(`shapes`)을 하나씩 넣어 봅니다. 이미 `chosen` 에 있는 조합은 건너뜁니다.
    //      넣어 본 팀 = `team` + 그 자리를 뺀 나머지 `chosen_mons` + 후보 조합의 `virtual_mon`.
    //      위 욕심쟁이에서 `trial` 을 만든 방식과 거의 같고, 다른 건 "i 번째만 뺀다"는 점뿐입니다.
    //   5. 넣어 본 팀의 `team_key` 가 4번 이전에 구해 둔 값보다 작으면(작을수록 좋음)
    //      `chosen` 과 `chosen_mons` 의 그 자리를 후보 조합으로 바꾸고, 바뀌었다고 표시하세요.
    //      같은 자리에서 계속 찾을 거라면, 비교 기준값도 새 점수로 바꿔야 합니다.
    //
    //   막히기 쉬운 곳:
    //   - "i 번째만 빼고 모으기"는 `iter()` 에 `enumerate()` 를 붙인 뒤 `filter` 로 거르는 걸 생각해 보세요.
    //   - `trial` 이 `chosen_mons` 를 빌리고 있는 동안에는 `chosen_mons` 를 못 바꿉니다.
    //     점수만 구하고 `trial` 을 다 쓴 다음에 바꾸세요. 컴파일러가 알려 줄 겁니다.
    //   - 안 끝나지 않을까 걱정할 필요는 없습니다. 바꿀 때마다 점수가 엄격히 작아지고 가능한 점수는 유한합니다.
    //
    // 확인: 아래 tests 의 `다듬은_뒤에는_…` 테스트에서 `#[ignore]` 를 지우고 `cargo test -p poketry`.

    // TODO(C4단계 ① 라): 고른 타입 조합마다 그 조합의 실제 포켓몬을 추천하세요.
    //   - `recommend(team, pokedex)` 를 **한 번만** 불러 둡니다(원래 팀 기준 순위).
    //   - `chosen` 의 조합 `shape` 마다: 추천 중 `shape_of(r.candidate) == *shape` 인 것만 걸러 `.cloned()` 로 복사해
    //     `SlotPick { slot_types: shape.clone(), picks }` 로 담습니다. `iter().map(..).collect()` 모양이에요.
    //   - 6마리면 `empty` 가 0 이라 `chosen` 이 비고, 결과도 자연히 빈 Vec 입니다.
    //
    // 확인: 아래 tests 의 `빈자리_…`·`팀이_다_차면_…` 두 테스트에서 `#[ignore]` 를 지우고 `cargo test -p poketry`.
    let _ = (chosen, pokedex);
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
    fn 빈자리_수만큼_서로_다른_타입_조합을_고르고_그_조합_포켓몬만_추천한다() {
        let team = find_team(&["리자몽", "가디안", "루카리오"]).unwrap();
        let slots = recommend_slots(&team, pokedex());

        assert_eq!(slots.len(), 3);
        for (i, slot) in slots.iter().enumerate() {
            assert!(
                !slot.picks.is_empty(),
                "{:?} 추천이 비었습니다",
                slot.slot_types
            );
            assert!(
                slots[..i].iter().all(|s| s.slot_types != slot.slot_types),
                "같은 타입 조합을 두 번 골랐습니다"
            );
            for pick in &slot.picks {
                assert_eq!(shape_of(pick.candidate), slot.slot_types);
                assert!(!team.iter().any(|t| t.id == pick.candidate.id));
            }
        }
    }

    #[test]
    #[ignore]
    fn 다듬은_뒤에는_한_자리만_바꿔서_나아지는_조합이_없다() {
        for names in [
            &["리자몽"][..],
            &["리자몽", "가디안"],
            &["리자몽", "가디안", "루카리오"],
            &["갸라도스", "마기라스", "님피아", "루카리오"],
        ] {
            let team = find_team(names).unwrap();
            let slots = recommend_slots(&team, pokedex());
            let chosen: Vec<Vec<Type>> = slots.iter().map(|s| s.slot_types.clone()).collect();
            let mons: Vec<Pokemon> = chosen.iter().map(|s| virtual_mon(s)).collect();

            let mut full = team.clone();
            full.extend(mons.iter());
            let current = team_key(&full);

            let shapes = type_shapes(
                pokedex()
                    .iter()
                    .filter(|p| !team.iter().any(|t| t.id == p.id)),
            );
            for i in 0..chosen.len() {
                for shape in shapes.iter().filter(|s| !chosen.contains(s)) {
                    let other = virtual_mon(shape);
                    let mut trial = team.clone();
                    trial.extend(
                        mons.iter()
                            .enumerate()
                            .filter(|(j, _)| *j != i)
                            .map(|(_, m)| m),
                    );
                    trial.push(&other);
                    assert!(
                        team_key(&trial) >= current,
                        "{names:?}: {i}번 자리를 {shape:?} 로 바꾸면 더 좋아집니다"
                    );
                }
            }
        }
    }

    #[test]
    fn 타입_조합은_순서와_상관없이_하나로_센다() {
        let shapes = type_shapes(pokedex());
        assert!(shapes.contains(&shape_of(&mon(Type::Fire, Some(Type::Flying)))));
        assert!(!shapes.contains(&vec![Type::Flying, Type::Fire]));
        assert!(shapes.iter().all(|s| s.len() == 1 || s.len() == 2));
    }

    #[test]
    #[ignore]
    fn 팀이_다_차면_빈자리_추천은_없다() {
        let team = find_team(&[
            "리자몽",
            "가디안",
            "루카리오",
            "갸라도스",
            "마기라스",
            "님피아",
        ])
        .unwrap();
        assert!(recommend_slots(&team, pokedex()).is_empty());
    }

    #[test]
    #[ignore]
    fn 약점_타입마다_그_타입을_견디는_교체만_추천한다() {
        let team = find_team(&[
            "리자몽",
            "가디안",
            "루카리오",
            "갸라도스",
            "마기라스",
            "님피아",
        ])
        .unwrap();
        let spots = weak_spot(&team_weakness(&team));
        let fixes = recommend_fixes(&team, pokedex());

        assert!(
            !spots.is_empty(),
            "테스트 팀에 약점이 있어야 의미가 있습니다"
        );
        assert_eq!(fixes.iter().map(|f| f.weak_type).collect::<Vec<_>>(), spots);
        for fix in &fixes {
            assert!(!fix.swaps.is_empty());
            for s in &fix.swaps {
                assert!(defense_multiplier(fix.weak_type, s.candidate) < 1.0);
            }
        }
    }
}
