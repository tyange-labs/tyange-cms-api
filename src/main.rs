// 아직 안 쓰는 함수·변수에 대한 경고를 끕니다. 6단계까지 끝내면 이 줄을 지우세요.
#![allow(unused)]

mod pokemon;
mod types;

use std::clone;

use pokemon::{Pokemon, find, load_pokedex};
use types::{Type, effectiveness};

const POKEDEX_PATH: &str = "pokemon.json";

fn main() {
    // ── 여기부터 뼈대: 데이터 읽기, 인자로 받은 이름을 포켓몬으로 바꾸기 ──
    let pokedex = match load_pokedex(POKEDEX_PATH) {
        Ok(list) => list,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let names: Vec<String> = std::env::args().skip(1).collect();
    if names.is_empty() {
        println!("사용법: cargo run -- 리자몽 가디안 루카리오 …");
        println!("\npokemon.json 에 등록된 포켓몬:");
        for p in &pokedex {
            println!("  {}", p.label());
        }
        return;
    }

    let mut team: Vec<&Pokemon> = Vec::new();
    for name in &names {
        match find(&pokedex, name) {
            Some(p) => team.push(p),
            None => {
                eprintln!("'{name}' 은(는) pokemon.json 에 없습니다.");
                std::process::exit(1);
            }
        }
    }

    println!("엔트리:");
    for p in &team {
        println!("  {}", p.label());
    }
    // ── 뼈대 끝 ──

    // TODO(3단계): team 의 첫 번째 포켓몬에 대해, 18개 공격 타입 각각의 배율을 출력해 보세요.
    // TODO(5단계): print_team_weakness(&team) 를 만들어 호출하세요.
    // TODO(6단계): team 이 5마리일 때 recommend(&team, &pokedex) 를 만들어 호출하세요.
}

/// [3단계 — 직접 작성]
/// `attack` 타입의 기술을 `defender` 가 맞았을 때의 배율.
///
/// 복합 타입이면 두 타입 각각의 배율을 곱합니다. (그래서 4.0 과 0.25 가 나옵니다)
/// 재료: `effectiveness(attack, 방어타입)` 과 `defender.types()`.
fn defense_multiplier(attack: Type, defender: &Pokemon) -> f32 {
    let first_multiplier = effectiveness(attack, defender.type1);

    if let Some(second_type) = defender.type2 {
        let second_multiplier = effectiveness(attack, second_type);

        return first_multiplier * second_multiplier;
    }

    first_multiplier
}

// [5단계 — 직접 작성] 팀 약점표
// 공격 타입마다 "약점(2배 이상)인 마릿수"와 "반감·무효(1배 미만)인 마릿수"를 세어 출력합니다.
// fn print_team_weakness(team: &[&Pokemon]) { … }

// [6단계 — 직접 작성] 빈자리 추천
// pokedex 에서 team 에 없는 포켓몬을 한 마리씩 넣어 보고, 점수가 좋은 3마리를 출력합니다.
// 점수 공식은 직접 정합니다.
// fn recommend(team: &[&Pokemon], pokedex: &[Pokemon]) { … }

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(type1: Type, type2: Option<Type>) -> Pokemon {
        Pokemon {
            name: "테스트".to_string(),
            type1,
            type2,
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
}
