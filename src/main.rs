mod display;
mod pokemon;
mod types;

use display::{pad_left, pad_right};

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

    print_team_weakness(&team);
    if team.len() == 5 {
        println!("\n빈자리 추천:");
        for (i, p) in recommend(&team, &pokedex).iter().enumerate() {
            println!("  {}. {}", i + 1, p.label());
        }
    }
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

#[derive(Debug)]
struct Weakness {
    attack_type: Type,
    under_multiplier_count: i32,
    twice_multiplier_count: i32,
}

// [5단계 — 직접 작성] 팀 약점표
// 공격 타입마다 "약점(2배 이상)인 마릿수"와 "반감·무효(1배 미만)인 마릿수"를 세어 출력합니다.

fn team_weakness(team: &[&Pokemon]) -> Vec<Weakness> {
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

fn print_team_weakness(team: &[&Pokemon]) {
    // 출력 정렬은 Claude 담당: 열 폭은 화면 칸 수 기준입니다.
    println!("\n팀 약점표:");
    println!(
        "  {}{}{}",
        pad_right("공격", 8),
        pad_left("약점", 6),
        pad_left("반감/무효", 12)
    );
    for w in team_weakness(team) {
        // 약점인 팀원이 있는데 받아 줄 팀원이 없거나, 받아 줄 팀원보다 약점이 많으면 표시합니다.
        let weak = w.twice_multiplier_count;
        let resist = w.under_multiplier_count;
        let mark = if weak > 0 && resist == 0 {
            "  ◀ 받아 줄 팀원 없음"
        } else if weak > resist {
            "  ◀ 위험"
        } else {
            ""
        };
        println!(
            "  {}{:>6}{:>12}{mark}",
            pad_right(w.attack_type.name(), 8),
            w.twice_multiplier_count,
            w.under_multiplier_count,
        );
    }
}

// [6단계 — 직접 작성] 빈자리 추천
// pokedex 에서 team 에 없는 포켓몬을 한 마리씩 넣어 보고, 점수가 좋은 3마리를 출력합니다.
// 점수 공식은 직접 정합니다.
fn recommend<'a>(team: &[&Pokemon], pokedex: &'a [Pokemon]) -> Vec<&'a Pokemon> {
    let mut result: Vec<(i32, &Pokemon)> = Vec::new();

    let candidates: Vec<&Pokemon> = pokedex
        .iter()
        .filter(|p| !team.iter().any(|t| t.name == p.name))
        .collect();

    for candidate in candidates {
        let mut new_team = team.to_vec();
        new_team.push(candidate);
        let weaknesses = team_weakness(&new_team);
        let score = team_score(&weaknesses);
        result.push((score, candidate));
    }

    result.sort_by_key(|k| -k.0);
    result.into_iter().map(|r| r.1).take(3).collect()
}

fn team_score(weakness: &[Weakness]) -> i32 {
    let mut result = 0;

    for weak in weakness {
        result = result + (weak.under_multiplier_count - weak.twice_multiplier_count);
    }

    result
}

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
