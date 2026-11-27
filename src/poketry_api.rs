use poem::http::StatusCode;
use poem_openapi::{payload::Json, Object, OpenApi};
use poketry::{
    find_team, find_team_by_ids, pokedex, recommend_fixes, recommend_slots, team_score,
    team_weakness, weak_spot, Pokemon,
};

use crate::models::ApiResponse;

#[derive(Debug, Object)]
#[oai(rename_all = "snake_case")]
pub struct AnalyzeRequest {
    pub names: Option<Vec<String>>,
    pub ids: Option<Vec<u32>>,
    pub limit: Option<u32>,
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
    pub weak_weight: i32,
    pub resist_weight: i32,
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
    pub covers: Vec<String>,
    pub spot_increase_if_removed: i32,
    pub picks: Vec<PokemonItem>,
}

#[derive(Debug, Object)]
#[oai(rename_all = "snake_case")]
pub struct FixItem {
    pub weak_type: String,
    pub swaps: Vec<SwapItem>,
}

const DEFAULT_LIMIT: u32 = 10;
const MAX_LIMIT: u32 = 50;
const MAX_TEAM: usize = 6;

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
            covers: s.covers.iter().map(|t| t.name().to_string()).collect(),
            spot_increase_if_removed: s.spot_increase_if_removed,
            picks: s.picks.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<poketry::WeakSpotFix<'_>> for FixItem {
    fn from(f: poketry::WeakSpotFix<'_>) -> Self {
        FixItem {
            weak_type: f.weak_type.name().to_string(),
            swaps: f.swaps.into_iter().map(Into::into).collect(),
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
            weak_weight: w.weak_weight,
            resist_weight: w.resist_weight,
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
        let limit = body.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(bad_request(format!("limit 은 1~{MAX_LIMIT} 사이로 보내 주세요.")));
        }
        let limit = limit as usize;

        let team = match (&body.names, &body.ids) {
            (Some(names), None) => {
                check_team_size(names.len())?;
                find_team(names).map_err(poem::error::BadRequest)?
            }
            (None, Some(ids)) => {
                check_team_size(ids.len())?;
                find_team_by_ids(ids).map_err(poem::error::BadRequest)?
            }
            _ => return Err(bad_request("names 와 ids 중 하나만 보내 주세요.")),
        };
        check_no_duplicates(&team)?;

        let weakness = team_weakness(&team);
        let spots = weak_spot(&weakness);
        let score = team_score(&weakness);

        let (mut plan, mut fixes) = if team.len() == MAX_TEAM {
            (None, recommend_fixes(&team, pokedex()))
        } else {
            (Some(recommend_slots(&team, pokedex())), Vec::new())
        };
        for slot in plan.iter_mut().flat_map(|p| p.slots.iter_mut()) {
            slot.picks.truncate(limit);
        }
        for fix in &mut fixes {
            fix.swaps.truncate(limit);
        }

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

fn bad_request(message: impl Into<String>) -> poem::Error {
    poem::Error::from_string(message.into(), StatusCode::BAD_REQUEST)
}

fn check_team_size(len: usize) -> poem::Result<()> {
    if len == 0 || len > MAX_TEAM {
        return Err(bad_request(format!("1~{MAX_TEAM}마리를 보내 주세요.")));
    }
    Ok(())
}

fn check_no_duplicates(team: &[&Pokemon]) -> poem::Result<()> {
    for (i, p) in team.iter().enumerate() {
        if team[..i].iter().any(|q| q.id == p.id) {
            return Err(bad_request(format!("'{}' 이(가) 두 번 들어 있습니다.", p.name)));
        }
    }
    Ok(())
}
