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
