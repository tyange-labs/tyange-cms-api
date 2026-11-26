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
    pub picks: Vec<&'a Pokemon>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SlotPlan<'a> {
    pub slots: Vec<SlotPick<'a>>,
    pub spot_count: usize,
    pub score: i32,
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

pub fn recommend_slots<'a>(team: &[&Pokemon], pokedex: &'a [Pokemon]) -> SlotPlan<'a> {
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

    loop {
        let mut changed = false;

        for i in 0..chosen.len() {
            let mut current: Vec<&Pokemon> = team.to_vec();
            current.extend(chosen_mons.iter());
            let mut best_key = team_key(&current);

            for shape in &shapes {
                if chosen.contains(shape) {
                    continue;
                }

                let candidate = virtual_mon(shape);
                let key = {
                    let mut trial: Vec<&Pokemon> = team.to_vec();
                    trial.extend(
                        chosen_mons
                            .iter()
                            .enumerate()
                            .filter(|c| i != c.0)
                            .map(|c| c.1),
                    );
                    trial.push(&candidate);
                    team_key(&trial)
                };

                if key < best_key {
                    chosen[i] = shape.clone();
                    chosen_mons[i] = candidate;
                    best_key = key;
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }

    // TODO(C4단계 ① 라): 고른 타입 조합으로 결과(`SlotPlan`)를 만드세요.
    //   점수는 자리마다가 아니라 "빈자리를 다 채운 팀" 하나에 붙고, 자리마다에는 그 조합의 포켓몬 목록만 붙습니다.
    //   (같은 타입 조합의 포켓몬은 상성 점수가 전부 같아서, 자리 안에서 점수로는 순서를 못 정합니다.)
    //
    //   1. 최종 팀 점수: `team` + `chosen_mons` 전부로 `team_weakness` 를 구하고,
    //      `weak_spot(..).len()` 을 `spot_count` 에, `team_score(..)` 를 `score` 에 넣습니다.
    //      팀을 모으는 방법은 다듬기에서 `current` 를 만든 것과 같습니다.
    //   2. 자리마다 포켓몬: `chosen` 의 조합 `shape` 마다, 도감(`pokedex`)에서
    //      - 팀에 이미 있는 포켓몬(`id` 로 비교)은 빼고,
    //      - `shape_of(p) == *shape` 인 것만 남겨 `Vec<&Pokemon>` 으로 모은 뒤,
    //      - 포획률(`capture_rate`)이 높은 순으로 정렬합니다. 높은 순은 `recommend` 의 정렬에서 쓴 `Reverse` 를 떠올려 보세요.
    //      `SlotPick { slot_types: shape.clone(), picks }` 로 담아 `Vec<SlotPick>` 을 만듭니다.
    //   3. `SlotPlan { slots, spot_count, score }` 를 돌려줍니다.
    //   6마리면 `chosen` 이 비어서 `slots` 도 비고, 점수는 원래 팀 점수가 됩니다.
    //
    // 확인: 아래 tests 의 `#[ignore]` 붙은 테스트 중 `빈자리_…`·`다듬은_뒤에는_…`·`팀이_다_차면_…` 세 개에서
    //   `#[ignore]` 를 지우고 `cargo test -p poketry`.
    let _ = (team, chosen, chosen_mons, pokedex);
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
        let plan = recommend_slots(&team, pokedex());
        let slots = &plan.slots;

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
                assert_eq!(shape_of(pick), slot.slot_types);
                assert!(!team.iter().any(|t| t.id == pick.id));
            }
            assert!(
                slot.picks
                    .windows(2)
                    .all(|w| w[0].capture_rate >= w[1].capture_rate),
                "{:?} 포켓몬이 포획률 높은 순이 아닙니다",
                slot.slot_types
            );
        }

        let mons: Vec<Pokemon> = slots.iter().map(|s| virtual_mon(&s.slot_types)).collect();
        let mut full = team.clone();
        full.extend(mons.iter());
        let weaknesses = team_weakness(&full);
        assert_eq!(plan.spot_count, weak_spot(&weaknesses).len());
        assert_eq!(plan.score, team_score(&weaknesses));
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
            let plan = recommend_slots(&team, pokedex());
            let chosen: Vec<Vec<Type>> = plan.slots.iter().map(|s| s.slot_types.clone()).collect();
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
        let plan = recommend_slots(&team, pokedex());
        let weaknesses = team_weakness(&team);
        assert!(plan.slots.is_empty());
        assert_eq!(plan.spot_count, weak_spot(&weaknesses).len());
        assert_eq!(plan.score, team_score(&weaknesses));
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
