"""PokeAPI 에서 레전드 Z-A 도감(본편 + DLC)을 받아 pokemon.json 을 만듭니다.

실행: python3 scripts/fetch_pokedex.py [캐시_폴더]
- 도감: lumiose-city(본편), hyperspace(DLC 메가 디멘션). 겹치는 종은 한 번만.
- 종마다 기본 폼 1줄 + 메가진화 폼(이름에 "-mega")을 따로 넣습니다.
- 응답은 캐시 폴더에 저장해 두고, 다시 실행하면 재사용합니다.
"""

from __future__ import annotations

import json
import sys
import time
import urllib.request
from pathlib import Path

API = "https://pokeapi.co/api/v2"
DEXES = ["lumiose-city", "hyperspace"]
OUT = Path(__file__).resolve().parent.parent / "pokemon.json"

TYPE_KO = {
    "normal": "노말", "fire": "불꽃", "water": "물", "electric": "전기",
    "grass": "풀", "ice": "얼음", "fighting": "격투", "poison": "독",
    "ground": "땅", "flying": "비행", "psychic": "에스퍼", "bug": "벌레",
    "rock": "바위", "ghost": "고스트", "dragon": "드래곤", "dark": "악",
    "steel": "강철", "fairy": "페어리",
}

# PokeAPI 에 한국어 이름이 없어 직접 지정하는 폼들.
# 모습·색만 다르고 타입이 같은 폼은 같은 이름을 주면 아래에서 한 줄로 합쳐집니다.
NAME_OVERRIDES = {
    "meowstic-male-mega": "메가냐오닉스",
    "meowstic-female-mega": "메가냐오닉스",
    "tatsugiri-curly-mega": "메가싸리용",
    "tatsugiri-droopy-mega": "메가싸리용",
    "tatsugiri-stretchy-mega": "메가싸리용",
    "magearna-mega": "메가마기아나",
    "magearna-original-mega": "메가마기아나",
}

cache_dir =Path(sys.argv[1]) if len(sys.argv) > 1 else Path(".pokeapi-cache")
cache_dir.mkdir(parents=True, exist_ok=True)


def get(path: str) -> dict:
    """API 응답을 캐시에서 읽거나, 없으면 받아서 저장합니다."""
    file = cache_dir / (path.strip("/").replace("/", "__") + ".json")
    if file.exists():
        return json.loads(file.read_text())
    req = urllib.request.Request(f"{API}/{path}", headers={"User-Agent": "za-entry-fetch"})
    for attempt in range(5):  # 공개 API 가 가끔 연결을 끊어서 재시도합니다
        try:
            with urllib.request.urlopen(req, timeout=30) as res:
                data = json.load(res)
            break
        except Exception:
            if attempt == 4:
                raise
            time.sleep(2 ** attempt)
    file.write_text(json.dumps(data, ensure_ascii=False))
    time.sleep(0.1)  # 공개 API 이니 천천히
    return data


def ko(names: list) -> str | None:
    return next((n["name"] for n in names if n["language"]["name"] == "ko"), None)


def types_of(pokemon: dict) -> tuple[str, str | None]:
    ts = [TYPE_KO[t["type"]["name"]] for t in sorted(pokemon["types"], key=lambda t: t["slot"])]
    return ts[0], (ts[1] if len(ts) > 1 else None)


def entry(name: str, pokemon: dict, capture_rate: int) -> dict:
    t1, t2 = types_of(pokemon)
    e = {"name": name, "type1": t1}
    if t2:
        e["type2"] = t2
    # 포획률은 종(species) 정보라서 메가 폼도 원래 종과 같은 값을 씁니다.
    e["capture_rate"] = capture_rate
    return e


species_names: list[str] = []
for dex in DEXES:
    for e in get(f"pokedex/{dex}/")["pokemon_entries"]:
        s = e["pokemon_species"]["name"]
        if s not in species_names:
            species_names.append(s)

result, missing = [], []
for i, s in enumerate(species_names, 1):
    print(f"\r{i}/{len(species_names)} {s:<20}", end="", file=sys.stderr)
    species = get(f"pokemon-species/{s}/")
    base_name = ko(species["names"]) or s
    if base_name == s:
        missing.append(s)

    for v in species["varieties"]:
        vname = v["pokemon"]["name"]
        if v["is_default"]:
            result.append(entry(base_name, get(f"pokemon/{vname}/"), species["capture_rate"]))
        elif "-mega" in vname:
            pokemon = get(f"pokemon/{vname}/")
            form = get(f"pokemon-form/{pokemon['forms'][0]['name']}/")
            mega_name = NAME_OVERRIDES.get(vname) or ko(form["form_names"]) or ko(form["names"])
            if not mega_name:
                missing.append(vname)
                mega_name = f"메가{base_name}" + vname.split("-mega")[-1].replace("-", "").upper()
            result.append(entry(mega_name, pokemon, species["capture_rate"]))

print(file=sys.stderr)
# 이름과 타입이 모두 같은 줄은 하나만 남깁니다. 이름만 같고 타입이 다르면 멈춥니다.
seen: dict[str, dict] = {}
unique = []
for e in result:
    if e["name"] in seen:
        if seen[e["name"]] != e:
            sys.exit(f"이름은 같은데 타입이 다릅니다. NAME_OVERRIDES 를 고치세요: {e['name']}")
        continue
    seen[e["name"]] = e
    unique.append(e)
result = unique

OUT.write_text(
    "[\n" + ",\n".join("  " + json.dumps(e, ensure_ascii=False) for e in result) + "\n]\n"
)
print(f"{len(species_names)}종, {len(result)}줄 → {OUT}")
if missing:
    print(f"한국어 이름이 없어 대체한 항목 {len(missing)}개: {', '.join(missing)}")
