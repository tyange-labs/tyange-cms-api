use std::cmp::Reverse;

use serde::Serialize;

use crate::pokemon::Pokemon;
use crate::types::{Type, effectiveness};

// TODO(R4 ①): 타입 목록 하나가 공격 타입 하나를 몇 배로 받는지 계산하세요.
//   R4 전체 목표: 상성 계산이 `Pokemon` 대신 `[Type]` 만 받게 바꿔서, `recommend_slots` 가 타입 조합을 넣어 볼 때
//   가짜 포켓몬(`virtual_mon`)을 만들 필요가 없게 합니다. ①~⑥ 순서대로 하면 매 단계 `cargo test -p poketry` 가 통과합니다.
//
//   예: multiplier_for(얼음, &[풀, 비행]) = 2.0 × 2.0 = 4.0
//       multiplier_for(불꽃, &[풀])       = 2.0          (타입이 하나여도 같은 함수)
//       multiplier_for(물,   &[풀, 불꽃]) = 0.5 × 2.0 = 1.0
//
//   1. 곱해 나갈 값을 `let mut m = 1.0;` 으로 시작합니다(곱셈의 시작은 0 이 아니라 1).
//   2. `types` 의 타입 `t` 마다 `effectiveness(attack, t)` 를 `m` 에 곱합니다(`m *= …`).
//   3. 다 돌면 `m` 을 돌려줍니다.
//
//   for 로 쓰면:
//     let mut m = 1.0;
//     for t in types {
//         m *= /* attack 이 t 를 칠 때 배율 */;
//     }
//     m
//   막히기 쉬운 곳: `types` 는 `&[Type]` 라서 `for t in types` 의 `t` 는 `&Type` 입니다. `effectiveness` 는 `Type` 값을 받으니
//   `*t` 로 꺼내거나 처음부터 `for &t in types` 로 받으세요(`Type` 은 `Copy` 라 복사됩니다).
//   (선택) 한 줄 버전: `types.iter().map(|&t| effectiveness(attack, t)).product()`
//
// 확인: tests 의 `타입_목록으로_배율을_계산한다` 에서 `#[ignore]` 줄을 지우고 `cargo test -p poketry`.
pub fn multiplier_for(attack: Type, types: &[Type]) -> f32 {
    let _ = (attack, types);
    todo!()
}

// TODO(R4 ②): ①이 통과하면 이 함수 본문 전체를 `multiplier_for` 한 번 부르는 것으로 바꾸세요.
//   포켓몬에서 타입 목록을 꺼내는 건 `defender.types()` 입니다(`Vec<Type>`, 예: 리자몽 → `[불꽃, 비행]`).
//   `multiplier_for(attack, &defender.types())` — `&Vec<Type>` 은 `&[Type]` 자리에 그대로 들어갑니다.
//   지금 있는 `type1` / `if let Some(second_type)` 코드는 전부 지웁니다. 같은 일을 ①이 하니까요.
// 확인: `cargo test -p poketry` — 원래 있던 `단일_타입은_…`·`복합_타입은_곱한다`·`무효가_…` 테스트가 그대로 통과해야 합니다.
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

// TODO(R4 ③): 팀 약점표를 포켓몬 대신 "멤버마다 타입 목록"으로 만드세요.
//   바로 아래 `team_weakness` 와 하는 일이 똑같고, 입력만 다릅니다:
//     team_weakness(&[리자몽, 루카리오])           ← 포켓몬
//     weakness_of(&[vec![불꽃, 비행], vec![격투, 강철]]) ← 타입 목록
//   결과도 같은 모양입니다: 공격 타입 18개마다 `Weakness { attack_type, twice_multiplier_count, under_multiplier_count }`.
//
//   1. `team_weakness` 본문을 통째로 복사해 와서 시작하세요.
//   2. 바꿀 곳은 두 군데뿐입니다:
//      - `for p in team`                     → `for types in members`
//      - `defense_multiplier(attack, p)`     → `multiplier_for(attack, types)`
//   3. 나머지(2배 이상이면 weak, 1배 미만이면 resist, 18줄 push)는 그대로 둡니다.
//   막히기 쉬운 곳: `members` 는 `&[Vec<Type>]` 이라 `for types in members` 의 `types` 는 `&Vec<Type>` 입니다.
//   `multiplier_for` 는 `&[Type]` 을 받는데, `&Vec<Type>` 은 자동으로 `&[Type]` 이 되니 그대로 넘기면 됩니다.
//
// 확인: tests 의 `타입_목록으로_팀_약점표를_만든다` 에서 `#[ignore]` 줄을 지우고 `cargo test -p poketry`.
pub fn weakness_of(members: &[Vec<Type>]) -> Vec<Weakness> {
    let _ = members;
    todo!()
}

// TODO(R4 ④): ③이 통과하면 이 함수 본문을 "타입 목록으로 바꾼 뒤 `weakness_of` 부르기" 두 줄로 바꾸세요.
//   1. `team`(포켓몬 목록)을 `Vec<Vec<Type>>`(멤버마다 타입 목록)으로 바꿉니다.
//      예: [리자몽, 루카리오] → [[불꽃, 비행], [격투, 강철]]
//      하나씩 꺼내서(`iter`) 바꾸고(`map(|p| p.types())`) 모으기(`collect`). 변수 타입을 `Vec<Vec<Type>>` 로 적어 두면 `collect` 가 알아듣습니다.
//   2. `weakness_of(&그 변수)` 를 돌려줍니다.
//   지금 있는 `for attack in Type::ALL { … }` 는 ③으로 옮겨 갔으니 지웁니다.
// 확인: `cargo test -p poketry` — 전부 통과해야 합니다(`team_weakness` 를 쓰는 테스트가 많아서 이게 안전망입니다).
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

// TODO(R4 ⑤): 이 함수가 포켓몬 대신 타입 목록을 받게 바꾸세요. ⑥과 한 번에 해야 빌드가 됩니다(⑥이 이 함수를 부르니까요).
//   - 인자: `members: &[&Pokemon]` → `members: &[Vec<Type>]`
//   - 본문: `team_weakness(members)` → `weakness_of(members)`
//   나머지 줄은 그대로입니다.
fn team_key(members: &[&Pokemon]) -> (usize, i32) {
    let weaknesses = team_weakness(members);
    (weak_spot(&weaknesses).len(), -team_score(&weaknesses))
}

// TODO(R4 ⑥): `recommend_slots` 에서 이 함수를 더 부르지 않게 되면 함수째 지우세요.
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

// TODO(R4 ⑥): 빈자리 고르기(욕심쟁이·다듬기·최종 점수)가 포켓몬 대신 타입 목록만 다루게 바꾸세요.
//   지금은 타입 조합 `shape`(예: `[물, 땅]`)을 팀에 넣어 보려고 `virtual_mon(shape)` 으로 가짜 포켓몬을 만들고,
//   고른 조합을 `chosen`(타입)과 `chosen_mons`(가짜 포켓몬) 두 곳에 똑같이 들고 다닙니다.
//   ⑤로 `team_key` 가 타입 목록을 받게 되면, 팀도 타입 목록으로 만들어 두고 `shape` 를 그대로 넣으면 됩니다.
//
//   예 (팀 리자몽·가디안, 빈자리 4):
//     base  = [[불꽃, 비행], [에스퍼, 페어리]]        ← 팀을 타입 목록으로 (함수 맨 위에서 한 번)
//     trial = base + chosen + [shape]               ← 지금 `team.to_vec()` + `chosen_mons` + `&candidate` 자리
//
//   1. 함수 맨 위(`empty` 근처)에 `let base: Vec<Vec<Type>> = team.iter().map(|p| p.types()).collect();`
//   2. `chosen_mons` 를 만드는 줄을 지웁니다. 그다음 컴파일 에러가 나는 곳을 하나씩 아래 표대로 바꿉니다.
//        지금                                         →  바꾼 뒤
//        `let mut trial: Vec<&Pokemon> = team.to_vec();` →  `let mut trial: Vec<Vec<Type>> = base.clone();`
//        `let mut current: Vec<&Pokemon> = team.to_vec();` → `let mut current: Vec<Vec<Type>> = base.clone();`
//        `.extend(chosen_mons.iter())`                 →  `.extend(chosen.iter().cloned())`
//        `let candidate = virtual_mon(shape);` + `trial.push(&candidate);`  →  `trial.push(shape.clone());` 한 줄
//        다듬기의 `chosen_mons.iter().enumerate().filter(..).map(|c| c.1)`  →  `chosen` 으로, 끝을 `.map(|c| c.1.clone())`
//        `chosen_mons.push(..)` / `chosen_mons[i] = candidate;`          →  지움 (`chosen` 쪽 줄만 남김)
//   3. 맨 끝 최종 점수: `let mut v = team.to_vec(); v.extend(chosen_mons.iter());` 를
//      `let mut v = base.clone(); v.extend(chosen.iter().cloned());` 로, `team_weakness(&v)` 를 `weakness_of(&v)` 로.
//   4. 위의 `virtual_mon` 함수를 지웁니다.
//   막히기 쉬운 곳:
//   - `chosen.iter()` 는 `&Vec<Type>` 을 줍니다. `trial` 은 `Vec<Vec<Type>>`(값) 이라 `.cloned()` 로 복사해서 넣어야 합니다.
//   - `pokedex`·`filtered_pokedex`·`picks` 쪽(자리마다 실제 포켓몬 찾기)은 그대로 둡니다. 거기는 진짜 포켓몬이 필요해요.
//   - 다듬기에서 `chosen.contains(shape)` 로 읽은 뒤 `chosen[i] = shape.clone()` 으로 쓰는 건 지금처럼 그대로 됩니다.
//
// 확인: `virtual_mon` 함수까지 지운 상태로 `cargo test -p poketry` 가 전부 통과하면 R4 끝입니다.
//   (지운 `virtual_mon`·`chosen_mons` 를 어딘가에서 아직 쓰고 있으면 빌드부터 안 됩니다.)
//   테스트는 결과(어떤 조합을 골랐는지·점수)만 보니까, 결과가 하나라도 달라지면 실패합니다.
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
    let filtered_pokedex: Vec<&Pokemon> = pokedex
        .iter()
        .filter(|p| team.iter().all(|c| p.id != c.id))
        .collect();
    let mut slots: Vec<SlotPick> = Vec::new();
    for shape in &chosen {
        let mut picks: Vec<&Pokemon> = filtered_pokedex
            .iter()
            .filter(|fp| shape_of(fp) == *shape)
            .copied()
            .collect();
        picks.sort_by_key(|p| -(p.capture_rate as i32));
        slots.push(SlotPick {
            slot_types: shape.clone(),
            picks,
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

    fn mons_of(shapes: &[Vec<Type>]) -> Vec<Pokemon> {
        shapes
            .iter()
            .map(|s| mon(s[0], s.get(1).copied()))
            .collect()
    }

    fn key_of(members: &[&Pokemon]) -> (usize, i32) {
        let weaknesses = team_weakness(members);
        (weak_spot(&weaknesses).len(), -team_score(&weaknesses))
    }

    #[test]
    #[ignore = "R4 ①: multiplier_for 를 다 쓰면 이 줄을 지우세요"]
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
    #[ignore = "R4 ③: weakness_of 를 다 쓰면 이 줄을 지우세요"]
    fn 타입_목록으로_팀_약점표를_만든다() {
        let members = vec![vec![Type::Fire], vec![Type::Water, Type::Ground]];
        let table = weakness_of(&members);
        assert_eq!(table.len(), 18);

        let row = |attack: Type| {
            let w = table.iter().find(|w| w.attack_type == attack).unwrap();
            (w.twice_multiplier_count, w.under_multiplier_count)
        };
        assert_eq!(row(Type::Water), (1, 0));
        assert_eq!(row(Type::Grass), (1, 1));
        assert_eq!(row(Type::Electric), (0, 1));

        let team = find_team(&["리자몽", "가디안", "루카리오", "갸라도스", "마기라스"]).unwrap();
        let types: Vec<Vec<Type>> = team.iter().map(|p| p.types()).collect();
        let counts = |table: Vec<Weakness>| -> Vec<(Type, i32, i32)> {
            table
                .into_iter()
                .map(|w| {
                    (
                        w.attack_type,
                        w.twice_multiplier_count,
                        w.under_multiplier_count,
                    )
                })
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

        let chosen: Vec<Vec<Type>> = slots.iter().map(|s| s.slot_types.clone()).collect();
        let mons = mons_of(&chosen);
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
            let mons = mons_of(&chosen);

            let mut full = team.clone();
            full.extend(mons.iter());
            let current = key_of(&full);

            let shapes = type_shapes(
                pokedex()
                    .iter()
                    .filter(|p| !team.iter().any(|t| t.id == p.id)),
            );
            for i in 0..chosen.len() {
                for shape in shapes.iter().filter(|s| !chosen.contains(s)) {
                    let other = mon(shape[0], shape.get(1).copied());
                    let mut trial = team.clone();
                    trial.extend(
                        mons.iter()
                            .enumerate()
                            .filter(|(j, _)| *j != i)
                            .map(|(_, m)| m),
                    );
                    trial.push(&other);
                    assert!(
                        key_of(&trial) >= current,
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
