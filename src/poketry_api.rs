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
        // TODO(D단계 ②): 핸들러 본문을 쓰세요. 요청 `{"names": [...]}` 를 받아 분석 결과를 돌려줍니다.
        //
        // 1. 마릿수 검사: `body.names` 가 비었거나 6마리를 넘으면 400.
        //    400 응답은 이 저장소의 다른 라우트와 같은 방식입니다:
        //      `return Err(poem::Error::from_string("1~6마리를 보내 주세요.", StatusCode::BAD_REQUEST));`
        //    (`body.names` 를 바로 쓸 수 있는 건 `Json<T>` 가 안쪽 값으로 자동 역참조(`Deref`)돼서예요)

        if body.names.is_empty() || body.names.len() > 6 {
            return Err(poem::Error::from_string(
                "1~6마리를 보내 주세요.",
                StatusCode::BAD_REQUEST,
            ));
        }

        //
        // 2. 이름 → 포켓몬: `poketry::find_team(&body.names)` 는 `Result<Vec<&Pokemon>, UnknownPokemon>` 을 줍니다.
        //    도감에 없는 이름이면 `Err` 인데, 이걸 400 으로 바꿔서 바로 돌려보내세요.
        //    힌트: `Result` 의 `map_err` 로 에러를 `poem::Error` 로 바꾸고 끝에 `?` 를 붙이면,
        //    에러일 때 함수가 거기서 바로 끝납니다. 메시지는 `e.to_string()`
        //    (`UnknownPokemon` 이 `Display` 를 구현해서 "'xx' 은(는) 도감에 없습니다." 가 됩니다).

        let team = find_team(&body.names).map_err(poem::error::BadRequest)?;

        //
        // 3. 계산: `poketry::team_weakness(&team)` 로 약점표를 만들고, 그걸로 `poketry::weak_spot(..)`·`poketry::team_score(..)`.

        let weakness = team_weakness(&team);
        let spots = weak_spot(&weakness);
        let score = team_score(&weakness);
        //
        // 4. 추천 (poketry C4단계 함수): 6마리면 `poketry::recommend_fixes(&team, poketry::pokedex())`
        //    (약점 타입별 교체 후보), 그보다 적으면 `poketry::recommend_slots(..)`(빈자리별 타입 조합 + 그 조합 포켓몬,
        //    점수는 빈자리를 다 채운 팀 기준 하나). 둘 중 하나만 채웁니다: `plan` 은 `None`/`Some(..)`, `fixes` 는 빈 Vec/채운 Vec.
        //    `if team.len() == 6 { … } else { … }` 로 (plan, fixes) 두 개를 한 번에 만들면 깔끔합니다.
        //    그룹마다 몇 개까지 줄지는 `SlotItem`·`FixItem` 의 `From` 이 `PICKS_PER_GROUP` 으로 잘라 줍니다.
        //
        // 5. 응답으로 옮겨 담기: 결과 타입마다 `From` 이 있어서(①과 이 파일 위쪽) `.into()` 로 바뀝니다.
        //    `From` 이 참조(`&T`)를 받는지 값(`T`)을 받는지 보고 `iter()` 와 `into_iter()` 중에 고르세요.
        //    `Pokemon`·`Weakness` 는 참조로, `SlotPlan`·`WeakSpotFix` 는 값으로 받게 만들어 뒀습니다.
        //    `Option` 안의 값을 바꿀 때는 `Option` 의 `map` 을 쓰면 됩니다.
        //    `weak_spots` 는 `Vec<Type>` → `Vec<String>` (①의 types 와 같은 방법).
        //    마지막은 `Ok(Json(ApiResponse { status: true, data: Some(AnalyzeResponse { … }), message: None }))`.
        //
        // 확인: `cargo build` 가 되면 이 단계는 끝입니다. 실제 요청은 다음 단계(E)에서 서버에 등록한 뒤 보냅니다.
        //
        // 생각해 볼 거리 (선택):
        // - 같은 이름이 두 번 들어오면? 지금은 막지 않습니다. 400 으로 막을지 정해 보세요.
        // - 그룹당 개수를 요청의 `limit: Option<u32>` 로 받기, 이름 대신 `ids: Vec<u32>` 로 받기
        //   (GUI 가 붙으면 id 쪽이 안전합니다 — 이름은 표시용).
        let _ = (body, StatusCode::BAD_REQUEST);
        todo!()
    }
}
