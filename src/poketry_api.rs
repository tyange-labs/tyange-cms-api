use poem::http::StatusCode;
use poem_openapi::{payload::Json, Object, OpenApi};
use poketry::{
    find_team, pokedex, recommend_fixes, recommend_slots, team_score, team_weakness, weak_spot,
};

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
        PokemonItem {
            id: p.id,
            species_id: p.species_id,
            name: p.name.clone(),
            types: p.types().iter().map(|t| t.name().to_string()).collect(),
            capture_rate: p.capture_rate,
        }
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

// TODO(E단계): 이 API 를 서버에 등록하고 실제 요청으로 확인하세요. 지금은 `main.rs` 가 모르는 struct 라서
//   빌드할 때 "never constructed" 경고가 납니다(등록하면 사라집니다).
//
//   1. `src/main.rs` 위쪽 `use` 에 `use poketry_api::PoketryApi;` 를 넣습니다(`mod poketry_api;` 는 이미 있습니다).
//   2. `OpenApiService::new((PostsApi { … }, TagsApi { … }, …, CraftApi { … }, …), …)` 튜플 맨 끝에 `PoketryApi,` 를 넣습니다.
//      필드가 없는 struct 라 `PoketryApi { }` 가 아니라 이름만 씁니다.
//   3. 서버를 띄우고(`cargo run`, 포트는 `LISTEN_ADDR`·기본 8080) 요청을 보냅니다:
//        curl -s -X POST localhost:8080/poketry/analyze -H 'Content-Type: application/json' \
//          -d '{"names":["리자몽","가디안","루카리오","갸라도스","마기라스"]}' | jq
//      5마리면 `plan` 이 채워지고 `fixes` 는 `[]`, "님피아" 를 더해 6마리면 `plan` 이 `null` 이고 `fixes` 가 약점 타입마다 하나.
//      실패도 확인: `{"names":["없는몬"]}` → 400 "'없는몬' 은(는) 도감에 없습니다.", `{"names":[]}` → 400.
//      `/api-docs`(Swagger)에서도 보낼 수 있습니다.
//   4. 위 네 요청을 통합 테스트로 남깁니다. 모양은 `src/routes/portfolio_routes_test.rs` 처럼
//      `OpenApiService::new(PoketryApi, "test", "1.0")` 를 `poem::test::TestClient::new(..)` 에 바로 물리고
//      `.post("/poketry/analyze").body_json(&serde_json::json!({ "names": [...] })).send().await` 로 보냅니다.
//      확인할 것: 상태 코드, 5마리 → `data.plan` 있음·`data.fixes` 빔, 6마리 → 반대.
//      DB 가 필요 없는 API 라 `AppState`·`init_db` 준비는 필요 없습니다.
//   막히기 쉬운 곳: 테스트 파일은 `src/routes.rs` 에 `mod poketry_api_test;` 처럼 걸어야 돕니다(`mod portfolio_routes_test;` 줄 참고).
//
// 확인: 루트에서 `cargo build` 경고에서 `poketry_api` 가 사라지고, `cargo test` 가 통과하면 끝.
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

        let (plan, fixes) = if team.len() == 6 {
            (None, recommend_fixes(&team, pokedex()))
        } else {
            (Some(recommend_slots(&team, pokedex())), Vec::new())
        };

        Ok(Json(ApiResponse {
            status: true,
            data: Some(AnalyzeResponse {
                team: team.into_iter().map(Into::into).collect(),
                weaknesses: weakness.iter().map(Into::into).collect(),
                weak_spots: spots.iter().map(|t| t.name().to_string()).collect(),
                score,
                plan: plan.map(Into::into),
                fixes: fixes.into_iter().map(Into::into).collect(),
            }),
            message: None,
        }))
    }
}
