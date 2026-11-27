use std::cmp::Reverse;

use serde::Serialize;

use crate::pokemon::Pokemon;
use crate::types::{Type, effectiveness};

// TODO(R4): 상성 계산을 포켓몬이 아니라 타입 기준으로 바꾸기. 계획은 CLAUDE.md "poketry 이어서 하기" 백로그 R4.
pub fn defense_multiplier(attack: Type, defender: &Pokemon) -> f32 {
    let first_multiplier = effectiveness(attack, defender.type1);

    if let Some(second_type) = defender.type2 {
        let second_multiplier = effectiveness(attack, second_type);

        return first_multiplier * second_multiplier;
    }

    first_multiplier
}

// TODO(R5): 4배 약점·무효를 2배·반감보다 무겁게 세기. 계획은 CLAUDE.md "poketry 이어서 하기" 백로그 R5.
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

// TODO(R7): C4부터 `recommend`·`Recommendation`은 C2 테스트만 쓴다. 남길지 지울지 정하기. CLAUDE.md "poketry 이어서 하기" 백로그 R7.
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
        // TODO(R2): 팀원 빼기를 `name` 대신 `id` 로 비교하기 (`t.id == p.id`).
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
        // TODO(R2): 팀원 빼기를 `name` 대신 `id` 로 비교하기 (`t.id == p.id`).
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

// TODO(R6): 자리마다 근거 붙이기(이 조합이 막아 주는 약점 타입, 빼면 늘어나는 구멍 수). CLAUDE.md "poketry 이어서 하기" 백로그 R6.
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

// TODO(R4): 타입 기준 상성으로 바꾸면 이 가상 포켓몬과 `chosen_mons` 가 필요 없어진다.
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

    let mut v = team.to_vec();
    v.extend(chosen_mons.iter());
    // TODO(R1): `fitlered_pokedex` → `filtered_pokedex` (오타), `pokedex.into_iter()` → `pokedex.iter()`.
    let fitlered_pokedex: Vec<&Pokemon> = pokedex
        .into_iter()
        .filter(|p| team.iter().all(|c| p.id != c.id))
        .collect();
    let mut slots: Vec<SlotPick> = Vec::new();
    for shape in &chosen {
        let mut picks: Vec<&Pokemon> = fitlered_pokedex
            .iter()
            .filter(|fp| shape_of(fp) == *shape)
            .copied()
            .collect();
        picks.sort_by_key(|p| -(p.capture_rate as i32));
        slots.push(SlotPick {
            slot_types: shape.clone(),
            // TODO(R1): `picks` 는 이미 소유한 Vec 이라 `clone()` 없이 그대로 넘기면 된다.
            picks: picks.clone(),
        });
    }
    let weaknesses = team_weakness(&v);
    let score = team_score(&weaknesses);
    let spot_count = weak_spot(&weaknesses).len();
    SlotPlan {
        slots,
        spot_count,
        score,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WeakSpotFix<'a> {
    pub weak_type: Type,
    pub swaps: Vec<Swap<'a>>,
}

pub fn recommend_fixes<'a>(team: &[&'a Pokemon], pokedex: &'a [Pokemon]) -> Vec<WeakSpotFix<'a>> {
    let weaknesses = team_weakness(team);
    let weak_spots = weak_spot(&weaknesses);
    let swaps = recommend_swap(team, pokedex);
    let mut fixes: Vec<WeakSpotFix> = Vec::new();
    for w in weak_spots {
        let mut new_swaps: Vec<Swap> = Vec::new();
        for s in &swaps {
            if defense_multiplier(w, s.candidate) < 1.0 {
                new_swaps.push(s.clone())
            }
        }
        fixes.push(WeakSpotFix {
            weak_type: w,
            swaps: new_swaps,
        })
    }
    fixes
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

    // TODO(R3): `recommend` 결과 개수 = 도감 수 − 팀 수, `recommend_swap` 결과 개수 = 팀 수 × (도감 수 − 팀 수) 테스트 추가.
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
            // TODO(R2): 위 `recommend` 와 함께 `id` 비교로.
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
