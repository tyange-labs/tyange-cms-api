use std::cmp::Reverse;

use serde::Serialize;

use crate::pokemon::Pokemon;
use crate::types::{Type, effectiveness};

pub fn multiplier_for(attack: Type, types: &[Type]) -> f32 {
    let mut m = 1.0;
    for t in types {
        m *= effectiveness(attack, *t);
    }

    m
}

pub fn defense_multiplier(attack: Type, defender: &Pokemon) -> f32 {
    multiplier_for(attack, &defender.types())
}

#[derive(Debug, Clone, Serialize)]
pub struct Weakness {
    pub attack_type: Type,
    pub resist_weight: i32,
    pub weak_weight: i32,
}

pub fn weakness_of(members: &[Vec<Type>]) -> Vec<Weakness> {
    let mut result: Vec<Weakness> = Vec::new();

    for attack in Type::ALL {
        let mut weak = 0;
        let mut resist = 0;

        for types in members {
            let m = multiplier_for(attack, types);
            if m >= 4.0 {
                weak += 2;
            } else if m >= 2.0 {
                weak += 1;
            } else if m == 0.0 {
                resist += 3;
            } else if m <= 0.25 {
                resist += 2;
            } else if m < 1.0 {
                resist += 1;
            }
        }

        result.push(Weakness {
            attack_type: attack,
            weak_weight: weak,
            resist_weight: resist,
        });
    }

    result
}

pub fn team_weakness(team: &[&Pokemon]) -> Vec<Weakness> {
    let weakness: Vec<Vec<Type>> = team.iter().map(|p| p.types()).collect();
    weakness_of(&weakness)
}

pub fn weak_spot(weaknesses: &[Weakness]) -> Vec<Type> {
    let mut result: Vec<Type> = Vec::new();

    for weakness in weaknesses {
        if weakness.weak_weight > weakness.resist_weight {
            result.push(weakness.attack_type);
        }
    }

    result
}

pub fn team_score(weakness: &[Weakness]) -> i32 {
    let mut result = 0;

    for weak in weakness {
        result = result + (weak.resist_weight - weak.weak_weight);
    }

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
        .filter(|p| !team.iter().any(|t| t.id == p.id))
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
    pub covers: Vec<Type>,
    pub spot_increase_if_removed: i32,
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

fn team_key(members: &[Vec<Type>]) -> (usize, i32) {
    let weaknesses = weakness_of(members);
    (weak_spot(&weaknesses).len(), -team_score(&weaknesses))
}

pub fn recommend_slots<'a>(team: &[&Pokemon], pokedex: &'a [Pokemon]) -> SlotPlan<'a> {
    let base: Vec<Vec<Type>> = team.iter().map(|p| p.types()).collect();
    let empty = 6usize.saturating_sub(team.len());
    let shapes = type_shapes(
        pokedex
            .iter()
            .filter(|p| !team.iter().any(|t| t.id == p.id)),
    );

    let mut chosen: Vec<Vec<Type>> = Vec::new();

    for _ in 0..empty {
        let mut best: Option<(&Vec<Type>, (usize, i32))> = None;

        for shape in &shapes {
            if chosen.contains(shape) {
                continue;
            }

            let mut trial: Vec<Vec<Type>> = base.clone();
            trial.extend(chosen.iter().cloned());
            trial.push(shape.clone());

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
    }

    loop {
        let mut changed = false;

        for i in 0..chosen.len() {
            let mut current: Vec<Vec<Type>> = base.clone();
            current.extend(chosen.iter().cloned());
            let mut best_key = team_key(&current);

            for shape in &shapes {
                if chosen.contains(shape) {
                    continue;
                }

                let key = {
                    let mut trial: Vec<Vec<Type>> = base.clone();
                    trial.extend(
                        chosen
                            .iter()
                            .enumerate()
                            .filter(|c| i != c.0)
                            .map(|c| c.1.clone()),
                    );
                    trial.push(shape.clone());
                    team_key(&trial)
                };

                if key < best_key {
                    chosen[i] = shape.clone();
                    best_key = key;
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }

    let mut v = base.clone();
    v.extend(chosen.iter().cloned());
    let filtered_pokedex: Vec<&Pokemon> = pokedex
        .iter()
        .filter(|p| team.iter().all(|c| p.id != c.id))
        .collect();
    let team_spots = weak_spot(&weakness_of(&base));
    let full_spots = team_key(&v).0 as i32;
    let mut slots: Vec<SlotPick> = Vec::new();
    for (i, shape) in chosen.iter().enumerate() {
        let mut picks: Vec<&Pokemon> = filtered_pokedex
            .iter()
            .filter(|fp| shape_of(fp) == *shape)
            .copied()
            .collect();
        picks.sort_by_key(|p| -(p.capture_rate as i32));

        let covers: Vec<Type> = team_spots
            .iter()
            .copied()
            .filter(|&w| multiplier_for(w, shape) < 1.0)
            .collect();

        let mut without = base.clone();
        without.extend(
            chosen
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, c)| c.clone()),
        );
        let spot_increase_if_removed = team_key(&without).0 as i32 - full_spots;

        slots.push(SlotPick {
            slot_types: shape.clone(),
            picks,
            covers,
            spot_increase_if_removed,
        });
    }
    let weaknesses = weakness_of(&v);
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
            if defense_multiplier(w, s.candidate) < 1.0 && defense_multiplier(w, s.out) >= 1.0 {
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
    fn 타입_목록으로_배율을_계산한다() {
        assert_eq!(multiplier_for(Type::Fire, &[Type::Grass]), 2.0);
        assert_eq!(multiplier_for(Type::Ice, &[Type::Grass, Type::Flying]), 4.0);
        assert_eq!(multiplier_for(Type::Water, &[Type::Grass, Type::Fire]), 1.0);
        assert_eq!(
            multiplier_for(Type::Normal, &[Type::Ghost, Type::Dark]),
            0.0
        );
    }

    #[test]
    fn 약점과_내성은_배율_단계만큼_무겁게_센다() {
        let row = |attack: Type, types: Vec<Type>| {
            let table = weakness_of(&[types]);
            let w = table.iter().find(|w| w.attack_type == attack).unwrap();
            (w.weak_weight, w.resist_weight)
        };
        assert_eq!(row(Type::Ice, vec![Type::Grass, Type::Flying]), (2, 0));
        assert_eq!(row(Type::Fire, vec![Type::Grass]), (1, 0));
        assert_eq!(row(Type::Normal, vec![Type::Grass]), (0, 0));
        assert_eq!(row(Type::Water, vec![Type::Grass]), (0, 1));
        assert_eq!(row(Type::Grass, vec![Type::Fire, Type::Dragon]), (0, 2));
        assert_eq!(row(Type::Normal, vec![Type::Ghost]), (0, 3));
    }

    #[test]
    fn 타입_목록으로_팀_약점표를_만든다() {
        let members = vec![vec![Type::Fire], vec![Type::Water, Type::Ground]];
        let table = weakness_of(&members);
        assert_eq!(table.len(), 18);

        let row = |attack: Type| {
            let w = table.iter().find(|w| w.attack_type == attack).unwrap();
            (w.weak_weight, w.resist_weight)
        };
        assert_eq!(row(Type::Water), (1, 0));
        assert_eq!(row(Type::Grass), (2, 1));
        assert_eq!(row(Type::Electric), (0, 3));

        let team = find_team(&["리자몽", "가디안", "루카리오", "갸라도스", "마기라스"]).unwrap();
        let types: Vec<Vec<Type>> = team.iter().map(|p| p.types()).collect();
        let counts = |table: Vec<Weakness>| -> Vec<(Type, i32, i32)> {
            table
                .into_iter()
                .map(|w| (w.attack_type, w.weak_weight, w.resist_weight))
                .collect()
        };
        assert_eq!(counts(weakness_of(&types)), counts(team_weakness(&team)));
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
    fn recommend_swap_은_팀원마다_도감_전체를_후보로_본다() {
        let team = find_team(&[
            "리자몽",
            "가디안",
            "루카리오",
            "갸라도스",
            "마기라스",
            "님피아",
        ])
        .unwrap();
        let result = recommend_swap(&team, pokedex());

        assert_eq!(result.len(), team.len() * (pokedex().len() - team.len()));
        assert!(
            result
                .iter()
                .all(|s| team.iter().all(|t| t.id != s.candidate.id))
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
        let tied: Vec<&Swap> = swaps
            .iter()
            .take_while(|s| (s.spot_count, s.score) == (swaps[0].spot_count, swaps[0].score))
            .collect();

        assert!(tied.len() > 1, "1등과 동점인 교체가 있어야 의미가 있습니다");
        assert!(
            tied.windows(2)
                .all(|w| w[0].candidate.capture_rate >= w[1].candidate.capture_rate),
            "구멍 수·점수가 같으면 포획률 높은 순이어야 합니다"
        );
        assert!(
            tied.iter()
                .any(|s| s.candidate.capture_rate != tied[0].candidate.capture_rate),
            "동점 안에 포획률이 다른 교체가 있어야 의미가 있습니다"
        );
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

        let chosen: Vec<Vec<Type>> = slots.iter().map(|s| s.slot_types.clone()).collect();
        let mut full: Vec<Vec<Type>> = team.iter().map(|p| p.types()).collect();
        full.extend(chosen);
        let weaknesses = weakness_of(&full);
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
            let base: Vec<Vec<Type>> = team.iter().map(|p| p.types()).collect();

            let mut full = base.clone();
            full.extend(chosen.iter().cloned());
            let current = team_key(&full);

            let shapes = type_shapes(
                pokedex()
                    .iter()
                    .filter(|p| !team.iter().any(|t| t.id == p.id)),
            );
            for i in 0..chosen.len() {
                for shape in shapes.iter().filter(|s| !chosen.contains(s)) {
                    let mut trial = base.clone();
                    trial.extend(
                        chosen
                            .iter()
                            .enumerate()
                            .filter(|(j, _)| *j != i)
                            .map(|(_, c)| c.clone()),
                    );
                    trial.push(shape.clone());
                    assert!(
                        team_key(&trial) >= current,
                        "{names:?}: {i}번 자리를 {shape:?} 로 바꾸면 더 좋아집니다"
                    );
                }
            }
        }
    }

    #[test]
    fn 자리마다_막아_주는_약점과_빼면_늘어나는_구멍_수를_붙인다() {
        let team = find_team(&["리자몽", "가디안", "루카리오", "갸라도스", "마기라스"]).unwrap();
        let plan = recommend_slots(&team, pokedex());
        let slot = &plan.slots[0];

        assert_eq!(slot.slot_types, vec![Type::Ground, Type::Steel]);
        assert_eq!(slot.covers, vec![Type::Electric, Type::Rock]);
        assert_eq!(slot.spot_increase_if_removed, 2);
    }

    #[test]
    fn 자리별_근거는_원래_팀_약점과_나머지_자리로_다시_계산한_값과_같다() {
        for names in [
            &["리자몽"][..],
            &["리자몽", "가디안", "루카리오"],
            &["갸라도스", "마기라스", "님피아", "루카리오"],
        ] {
            let team = find_team(names).unwrap();
            let plan = recommend_slots(&team, pokedex());
            let base: Vec<Vec<Type>> = team.iter().map(|p| p.types()).collect();
            let team_spots = weak_spot(&weakness_of(&base));

            for (i, slot) in plan.slots.iter().enumerate() {
                let expected_covers: Vec<Type> = team_spots
                    .iter()
                    .copied()
                    .filter(|&w| multiplier_for(w, &slot.slot_types) < 1.0)
                    .collect();
                assert_eq!(slot.covers, expected_covers, "{names:?} {i}번 자리");

                let mut without = base.clone();
                for (j, other) in plan.slots.iter().enumerate() {
                    if j != i {
                        without.push(other.slot_types.clone());
                    }
                }
                let spots_without = weak_spot(&weakness_of(&without)).len() as i32;
                assert_eq!(
                    slot.spot_increase_if_removed,
                    spots_without - plan.spot_count as i32,
                    "{names:?} {i}번 자리"
                );
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
                assert!(
                    defense_multiplier(fix.weak_type, s.out) >= 1.0,
                    "{} 를 빼는 교체는 {:?} 약점을 줄이지 못합니다",
                    s.out.name,
                    fix.weak_type
                );
            }
        }
    }
}
