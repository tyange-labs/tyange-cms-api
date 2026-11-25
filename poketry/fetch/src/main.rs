//! PokeAPI 에서 레전드 Z-A 도감(본편 + DLC)을 받아 poketry 의 데이터 파일 두 개를 만듭니다.
//! (예전 `scripts/fetch_pokedex.py` 를 Rust 로 옮긴 것 — 저장소에 Python 을 두지 않으려고)
//!
//! 실행 (cms-api 루트에서): `cargo run -p poketry-fetch [캐시_폴더]`
//!
//! - `poketry/pokemon.json`: 종마다 기본 폼 1줄 + 메가진화 폼(이름에 "-mega")을 따로 넣습니다.
//!   도감: lumiose-city(본편), hyperspace(DLC 메가 디멘션). 겹치는 종은 한 번만.
//! - `poketry/evolutions.json`: 진화 계통(나무 모양). 도감의 종은 모두 정확히 한 계통에 들어갑니다.
//!   도감에 없는 진화 전·후 단계도 `in_dex: false` 로 넣습니다(화면에 계통을 통째로 보여주려고).
//!   메가진화는 진화 단계가 아니라 그 종의 폼이라 `megas` 에 이름만 붙입니다.
//! - 응답은 캐시 폴더(기본 `poketry/.pokeapi-cache`, git 제외)에 저장하고 다시 실행하면 재사용합니다.
//!
//! poketry 라이브러리와 달리 네트워크 의존성(reqwest·tokio)이 필요해서 별도 크레이트로 뒀습니다.
//! 서버 빌드(`cargo build` 루트)에는 들어가지 않습니다.

use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const API: &str = "https://pokeapi.co/api/v2/";
const DEXES: [&str; 2] = ["lumiose-city", "hyperspace"];

/// PokeAPI 에 한국어 이름이 없어 직접 지정하는 폼들.
/// 모습·색만 다르고 타입이 같은 폼은 같은 이름을 주면 한 줄로 합쳐집니다.
const NAME_OVERRIDES: [(&str, &str); 7] = [
    ("meowstic-male-mega", "메가냐오닉스"),
    ("meowstic-female-mega", "메가냐오닉스"),
    ("tatsugiri-curly-mega", "메가싸리용"),
    ("tatsugiri-droopy-mega", "메가싸리용"),
    ("tatsugiri-stretchy-mega", "메가싸리용"),
    ("magearna-mega", "메가마기아나"),
    ("magearna-original-mega", "메가마기아나"),
];

fn type_ko(en: &str) -> Result<&'static str> {
    Ok(match en {
        "normal" => "노말",
        "fire" => "불꽃",
        "water" => "물",
        "electric" => "전기",
        "grass" => "풀",
        "ice" => "얼음",
        "fighting" => "격투",
        "poison" => "독",
        "ground" => "땅",
        "flying" => "비행",
        "psychic" => "에스퍼",
        "bug" => "벌레",
        "rock" => "바위",
        "ghost" => "고스트",
        "dragon" => "드래곤",
        "dark" => "악",
        "steel" => "강철",
        "fairy" => "페어리",
        _ => return Err(format!("모르는 타입: {en}").into()),
    })
}

/// API 응답을 캐시에서 읽거나, 없으면 받아서 저장합니다.
struct Api {
    client: reqwest::Client,
    cache_dir: PathBuf,
}

impl Api {
    async fn get(&self, path: &str) -> Result<Value> {
        let file = self
            .cache_dir
            .join(format!("{}.json", path.trim_matches('/').replace('/', "__")));
        if let Ok(text) = std::fs::read_to_string(&file) {
            return Ok(serde_json::from_str(&text)?);
        }
        let url = format!("{API}{}", path.trim_start_matches('/'));
        let mut attempt = 0;
        let data: Value = loop {
            // 공개 API 가 가끔 연결을 끊어서 재시도합니다
            let res = async {
                self.client
                    .get(&url)
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<Value>()
                    .await
            }
            .await;
            match res {
                Ok(v) => break v,
                Err(e) if attempt == 4 => return Err(format!("{url}: {e:?}").into()),
                Err(_) => {
                    tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                    attempt += 1;
                }
            }
        };
        std::fs::write(&file, serde_json::to_string(&data)?)?;
        tokio::time::sleep(Duration::from_millis(100)).await; // 공개 API 이니 천천히
        Ok(data)
    }

    /// PokeAPI 가 돌려준 전체 URL(`https://pokeapi.co/api/v2/…`)로 받습니다.
    async fn get_url(&self, url: &str) -> Result<Value> {
        let path = url
            .strip_prefix(API)
            .ok_or_else(|| format!("PokeAPI 주소가 아닙니다: {url}"))?;
        self.get(path).await
    }
}

fn str_at<'a>(v: &'a Value, pointer: &str) -> Result<&'a str> {
    v.pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("응답에 {pointer} 가 없습니다").into())
}

fn arr_at<'a>(v: &'a Value, pointer: &str) -> Result<&'a Vec<Value>> {
    v.pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("응답에 {pointer} 가 없습니다").into())
}

fn ko(names: &Value) -> Option<String> {
    names.as_array()?.iter().find_map(|n| {
        (n.pointer("/language/name")?.as_str()? == "ko")
            .then(|| n.get("name")?.as_str().map(str::to_string))
            .flatten()
    })
}

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    name: String,
    type1: &'static str,
    type2: Option<&'static str>,
    capture_rate: u64,
}

impl Entry {
    fn new(name: String, pokemon: &Value, capture_rate: u64) -> Result<Self> {
        let mut types: Vec<(u64, &str)> = arr_at(pokemon, "/types")?
            .iter()
            .map(|t| Ok((t["slot"].as_u64().unwrap_or(0), str_at(t, "/type/name")?)))
            .collect::<Result<_>>()?;
        types.sort();
        Ok(Entry {
            name,
            type1: type_ko(types[0].1)?,
            type2: types.get(1).map(|t| type_ko(t.1)).transpose()?,
            // 포획률은 종(species) 정보라서 메가 폼도 원래 종과 같은 값을 씁니다.
            capture_rate,
        })
    }

    /// `{"name": "치코리타", "type1": "풀", "capture_rate": 45}` 모양 한 줄.
    /// (예전 Python 출력과 글자 하나까지 같게 — 바뀐 게 없으면 git diff 가 비어 있도록)
    fn to_line(&self) -> String {
        let q = |s: &str| serde_json::to_string(s).unwrap();
        let mut line = format!("{{\"name\": {}, \"type1\": {}", q(&self.name), q(self.type1));
        if let Some(t2) = self.type2 {
            line += &format!(", \"type2\": {}", q(t2));
        }
        line + &format!(", \"capture_rate\": {}}}", self.capture_rate)
    }
}

/// 진화 계통 나무를 한국어 이름으로 옮깁니다. `names` 는 종 영문명 → 한국어 이름.
fn chain_node(
    link: &Value,
    names: &HashMap<String, String>,
    in_dex: &HashSet<String>,
    megas: &HashMap<String, Vec<String>>,
    seen: &mut HashSet<String>,
) -> Result<Value> {
    let species = str_at(link, "/species/name")?;
    seen.insert(species.to_string());
    let evolves_to = arr_at(link, "/evolves_to")?
        .iter()
        .map(|next| chain_node(next, names, in_dex, megas, seen))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "name": names[species],
        "in_dex": in_dex.contains(species),
        "megas": megas.get(species).cloned().unwrap_or_default(),
        "evolves_to": evolves_to,
    }))
}

/// 계통 나무에 나오는 종 이름(영문)을 전부 모읍니다.
fn chain_species(link: &Value, out: &mut Vec<String>) -> Result<()> {
    out.push(str_at(link, "/species/name")?.to_string());
    for next in arr_at(link, "/evolves_to")? {
        chain_species(next, out)?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let poketry_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let cache_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| poketry_dir.join(".pokeapi-cache"));
    std::fs::create_dir_all(&cache_dir)?;
    let api = Api {
        client: reqwest::Client::builder()
            .user_agent("poketry-fetch")
            .timeout(Duration::from_secs(30))
            .build()?,
        cache_dir,
    };
    let overrides: HashMap<&str, &str> = NAME_OVERRIDES.into_iter().collect();

    // ── 1. 도감의 종 목록 ──
    let mut species_names: Vec<String> = Vec::new();
    for dex in DEXES {
        for e in arr_at(&api.get(&format!("pokedex/{dex}/")).await?, "/pokemon_entries")? {
            let s = str_at(e, "/pokemon_species/name")?.to_string();
            if !species_names.contains(&s) {
                species_names.push(s);
            }
        }
    }
    let in_dex: HashSet<String> = species_names.iter().cloned().collect();

    // ── 2. 종마다 기본 폼 + 메가 폼 ──
    let mut result: Vec<Entry> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    let mut ko_names: HashMap<String, String> = HashMap::new(); // 종 영문명 → 한국어 이름
    let mut mega_names: HashMap<String, Vec<String>> = HashMap::new();
    let mut chain_urls: Vec<String> = Vec::new(); // 처음 나온 순서대로

    for (i, s) in species_names.iter().enumerate() {
        eprint!("\r{}/{} {s:<20}", i + 1, species_names.len());
        let species = api.get(&format!("pokemon-species/{s}/")).await?;
        let base_name = ko(&species["names"]).unwrap_or_else(|| {
            missing.push(s.clone());
            s.clone()
        });
        ko_names.insert(s.clone(), base_name.clone());
        let capture_rate = species["capture_rate"]
            .as_u64()
            .ok_or("capture_rate 가 없습니다")?;
        if let Some(url) = species.pointer("/evolution_chain/url").and_then(Value::as_str) {
            if !chain_urls.iter().any(|u| u == url) {
                chain_urls.push(url.to_string());
            }
        }

        for v in arr_at(&species, "/varieties")? {
            let vname = str_at(v, "/pokemon/name")?;
            if v["is_default"].as_bool() == Some(true) {
                let pokemon = api.get(&format!("pokemon/{vname}/")).await?;
                result.push(Entry::new(base_name.clone(), &pokemon, capture_rate)?);
            } else if vname.contains("-mega") {
                let pokemon = api.get(&format!("pokemon/{vname}/")).await?;
                let form_name = str_at(&pokemon, "/forms/0/name")?;
                let form = api.get(&format!("pokemon-form/{form_name}/")).await?;
                let mega_name = overrides
                    .get(vname)
                    .map(|n| n.to_string())
                    .or_else(|| ko(&form["form_names"]))
                    .or_else(|| ko(&form["names"]))
                    .unwrap_or_else(|| {
                        missing.push(vname.to_string());
                        let suffix = vname.rsplit("-mega").next().unwrap_or("");
                        format!("메가{base_name}{}", suffix.replace('-', "").to_uppercase())
                    });
                let megas = mega_names.entry(s.clone()).or_default();
                if !megas.contains(&mega_name) {
                    megas.push(mega_name.clone());
                }
                result.push(Entry::new(mega_name, &pokemon, capture_rate)?);
            }
        }
    }
    eprintln!();

    // 이름과 타입이 모두 같은 줄은 하나만 남깁니다. 이름만 같고 타입이 다르면 멈춥니다.
    let mut seen: HashMap<String, Entry> = HashMap::new();
    let mut unique: Vec<Entry> = Vec::new();
    for e in result {
        match seen.get(&e.name) {
            Some(prev) if *prev != e => {
                return Err(format!(
                    "이름은 같은데 타입이 다릅니다. NAME_OVERRIDES 를 고치세요: {}",
                    e.name
                )
                .into());
            }
            Some(_) => continue,
            None => {
                seen.insert(e.name.clone(), e.clone());
                unique.push(e);
            }
        }
    }

    // ── 3. 진화 계통 ──
    let mut chains: Vec<Value> = Vec::new();
    let mut covered: HashSet<String> = HashSet::new();
    for (i, url) in chain_urls.iter().enumerate() {
        eprint!("\r진화 계통 {}/{}", i + 1, chain_urls.len());
        let chain = api.get_url(url).await?;
        // 도감에 없는 단계의 한국어 이름은 그 종 정보를 따로 받아 채웁니다.
        let mut members = Vec::new();
        chain_species(&chain["chain"], &mut members)?;
        for m in members {
            if !ko_names.contains_key(&m) {
                let species = api.get(&format!("pokemon-species/{m}/")).await?;
                let name = ko(&species["names"]).unwrap_or_else(|| {
                    missing.push(m.clone());
                    m.clone()
                });
                ko_names.insert(m, name);
            }
        }
        chains.push(chain_node(&chain["chain"], &ko_names, &in_dex, &mega_names, &mut covered)?);
    }
    eprintln!();
    let uncovered: Vec<&String> = species_names.iter().filter(|s| !covered.contains(*s)).collect();
    if !uncovered.is_empty() {
        return Err(format!("진화 계통에 없는 도감 종이 있습니다: {uncovered:?}").into());
    }

    // ── 4. 저장 ──
    let pokedex_out = poketry_dir.join("pokemon.json");
    let lines: Vec<String> = unique.iter().map(|e| format!("  {}", e.to_line())).collect();
    std::fs::write(&pokedex_out, format!("[\n{}\n]\n", lines.join(",\n")))?;

    let evo_out = poketry_dir.join("evolutions.json");
    std::fs::write(&evo_out, serde_json::to_string_pretty(&chains)? + "\n")?;

    println!(
        "{}종, {}줄 → {}",
        species_names.len(),
        unique.len(),
        pokedex_out.canonicalize()?.display()
    );
    println!("진화 계통 {}개 → {}", chains.len(), evo_out.canonicalize()?.display());
    if !missing.is_empty() {
        println!(
            "한국어 이름이 없어 대체한 항목 {}개: {}",
            missing.len(),
            missing.join(", ")
        );
    }
    Ok(())
}
