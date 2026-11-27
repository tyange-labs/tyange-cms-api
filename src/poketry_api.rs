use poem::http::StatusCode;
use poem_openapi::{payload::Json, Object, OpenApi};
use poketry::{find_team, team_score, team_weakness, weak_spot};

use crate::models::ApiResponse;

#[derive(Debug, Object)]
#[oai(rename_all = "snake_case")]
pub struct AnalyzeRequest {
    pub names: Vec<String>,
}

#[derive(Debug, Object)]
#[oai(rename_all = "snake_case")]
pub struct PokemonItem {
    pub id: u32,
    pub species_id: u32,
    pub name: String,
    pub types: Vec<String>,
    pub capture_rate: u8,
}

#[derive(Debug, Object)]
#[oai(rename_all = "snake_case")]
pub struct WeaknessRow {
    pub attack_type: String,
    pub weak_count: i32,
    pub resist_count: i32,
}

#[derive(Debug, Object)]
#[oai(rename_all = "snake_case")]
pub struct SwapItem {
    pub remove: PokemonItem,
    pub add: PokemonItem,
    pub spot_count: u32,
    pub score: i32,
}

#[derive(Debug, Object)]
#[oai(rename_all = "snake_case")]
pub struct AnalyzeResponse {
    pub team: Vec<PokemonItem>,
    pub weaknesses: Vec<WeaknessRow>,
    pub weak_spots: Vec<String>,
    pub score: i32,
    pub plan: Option<SlotPlanItem>,
    pub fixes: Vec<FixItem>,
}

#[derive(Debug, Object)]
#[oai(rename_all = "snake_case")]
pub struct SlotPlanItem {
    pub slots: Vec<SlotItem>,
    pub spot_count: u32,
    pub score: i32,
}

#[derive(Debug, Object)]
#[oai(rename_all = "snake_case")]
pub struct SlotItem {
    pub slot_types: Vec<String>,
    pub picks: Vec<PokemonItem>,
}

#[derive(Debug, Object)]
#[oai(rename_all = "snake_case")]
pub struct FixItem {
    pub weak_type: String,
    pub swaps: Vec<SwapItem>,
}

const PICKS_PER_GROUP: usize = 10;

impl From<poketry::SlotPlan<'_>> for SlotPlanItem {
    fn from(p: poketry::SlotPlan<'_>) -> Self {
        SlotPlanItem {
            slots: p.slots.into_iter().map(Into::into).collect(),
            spot_count: p.spot_count as u32,
            score: p.score,
        }
    }
}

impl From<poketry::SlotPick<'_>> for SlotItem {
    fn from(s: poketry::SlotPick<'_>) -> Self {
        SlotItem {
            slot_types: s.slot_types.iter().map(|t| t.name().to_string()).collect(),
            picks: s.picks.into_iter().take(PICKS_PER_GROUP).map(Into::into).collect(),
        }
    }
}

impl From<poketry::WeakSpotFix<'_>> for FixItem {
    fn from(f: poketry::WeakSpotFix<'_>) -> Self {
        FixItem {
            weak_type: f.weak_type.name().to_string(),
            swaps: f.swaps.into_iter().take(PICKS_PER_GROUP).map(Into::into).collect(),
        }
    }
}

impl From<&poketry::Pokemon> for PokemonItem {
    fn from(p: &poketry::Pokemon) -> Self {
        // TODO(D단계 ①): poketry 의 `Pokemon` 을 API 응답용 `PokemonItem` 으로 옮겨 담으세요.
        //
        // - `id`, `species_id`, `capture_rate` 는 숫자라 `p.id` 처럼 쓰면 그대로 복사됩니다.
        // - `name` 은 `String` 이라 그냥 `p.name` 으로는 못 가져옵니다. `p` 는 빌린 값(`&Pokemon`)이라
        //   안의 String 을 빼 갈 수 없어요. 복사본을 만드는 메서드를 쓰세요.
        // - `types` 가 핵심입니다. 계산용 `Pokemon` 은 `type1` + `type2: Option<Type>` 이지만,
        //   프론트에는 `["불꽃", "비행"]` 같은 문자열 배열이 쓰기 편해서 모양을 바꿉니다.
        //   재료: `p.types()` 는 `Vec<Type>`(1개 또는 2개)을 주고, `Type` 의 `.name()` 은 `"불꽃"` 같은 `&'static str` 을 줍니다.
        //   `Vec<Type>` → `Vec<String>` 은 "하나씩 꺼내서(iter) 바꾸고(map) 다시 모으기(collect)" 입니다.
        //   `&str` → `String` 은 `.to_string()`.
        //
        // 아래 `WeaknessRow` 의 `From` 이 같은 모양의 예시입니다.
        // 확인: `cargo build` 가 에러 없이 되면 됩니다. (실제로 부르는 건 ②에서)
        let _ = p;
        todo!()
    }
}

impl From<&poketry::Weakness> for WeaknessRow {
    fn from(w: &poketry::Weakness) -> Self {
        WeaknessRow {
            attack_type: w.attack_type.name().to_string(),
            weak_count: w.twice_multiplier_count,
            resist_count: w.under_multiplier_count,
        }
    }
}

impl From<poketry::Swap<'_>> for SwapItem {
    fn from(s: poketry::Swap<'_>) -> Self {
        SwapItem {
            remove: s.out.into(),
            add: s.candidate.into(),
            spot_count: s.spot_count as u32,
            score: s.score,
        }
    }
}

pub struct PoketryApi;

#[OpenApi]
impl PoketryApi {
    #[oai(path = "/poketry/analyze", method = "post")]
    async fn analyze(
        &self,
        body: Json<AnalyzeRequest>,
    ) -> poem::Result<Json<ApiResponse<AnalyzeResponse>>> {
        if body.names.is_empty() || body.names.len() > 6 {
            return Err(poem::Error::from_string(
                "1~6마리를 보내 주세요.",
                StatusCode::BAD_REQUEST,
            ));
        }

        let team = find_team(&body.names).map_err(poem::error::BadRequest)?;

        let weakness = team_weakness(&team);
        let spots = weak_spot(&weakness);
        let score = team_score(&weakness);

        // TODO(D단계 ②): 추천을 계산하고 응답에 옮겨 담으세요. 위 1~3(마릿수 검사·이름 → 포켓몬·약점 계산)은 됐습니다.
        //
        //   응답 예 (5마리 팀):  { team: [5마리], weaknesses: [18줄], weak_spots: ["물", "바위"], score: -3,
        //                          plan: Some({ slots: [{ slot_types: ["풀", "강철"], picks: [포켓몬…] }], spot_count, score }),
        //                          fixes: [] }
        //   6마리 팀이면 반대로 plan: None, fixes: [{ weak_type: "물", swaps: [교체…] }, …].
        //
        //   4. 추천: `if team.len() == 6 { … } else { … }` 로 `(plan, fixes)` 두 개를 한 번에 만듭니다.
        //      - 6마리: `(None, poketry::recommend_fixes(&team, poketry::pokedex()))`
        //      - 그보다 적으면: `(Some(poketry::recommend_slots(..)), Vec::new())` — 인자는 `recommend_slots` 시그니처를 보세요.
        //      `if` 의 두 갈래는 같은 타입이어야 해서, 이 단계에서는 poketry 타입(`SlotPlan`, `WeakSpotFix`) 그대로 두고
        //      5에서 API 타입으로 바꾸는 게 편합니다.
        //   5. 옮겨 담기: 결과 타입마다 `From` 이 있어서(①과 이 파일 위쪽) `.into()` 로 바뀝니다.
        //      - `team`: `Vec<&Pokemon>` → `Vec<PokemonItem>`. `From<&Pokemon>` 이라 꺼낸 원소(`&&Pokemon`)를 한 번 벗겨야 합니다
        //        (`team.into_iter()` 로 꺼내면 `&Pokemon` 이 나와요).
        //      - `weaknesses`: `From<&Weakness>` 라 `weakness.iter()` 로.
        //      - `weak_spots`: `Vec<Type>` → `Vec<String>` (①의 types 와 같은 방법).
        //      - `plan`: `Option<SlotPlan>` 안의 값을 바꿀 때는 `Option` 의 `map(Into::into)`.
        //      - `fixes`: `From<WeakSpotFix>`(값) 이라 `into_iter()` 로.
        //      - 마지막은 `Ok(Json(ApiResponse { status: true, data: Some(AnalyzeResponse { … }), message: None }))`.
        //   막히기 쉬운 곳: `team` 을 4에서 `&team` 으로 빌려 쓰는 동안에는 `into_iter()` 로 넘길 수 없어요.
        //   추천을 먼저 계산하고(4) 응답을 만들 때(5) 넘기면 순서가 맞습니다.
        //
        // 확인: 루트에서 `cargo build` 가 되면 이 단계는 끝입니다. 실제 요청은 다음 단계(E)에서 서버에 등록한 뒤 보냅니다.
        //
        // 생각해 볼 거리 (선택):
        // - 같은 이름이 두 번 들어오면? 지금은 막지 않습니다. 400 으로 막을지 정해 보세요.
        // - 그룹당 개수를 요청의 `limit: Option<u32>` 로 받기, 이름 대신 `ids: Vec<u32>` 로 받기
        //   (GUI 가 붙으면 id 쪽이 안전합니다 — 이름은 표시용).
        let _ = (spots, score);
        todo!()
    }
}
