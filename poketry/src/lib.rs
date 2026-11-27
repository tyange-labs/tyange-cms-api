pub mod analysis;
pub mod evolution;
pub mod pokemon;
pub mod types;

pub use analysis::{
    SlotPick, SlotPlan, Swap, WeakSpotFix, Weakness, defense_multiplier, multiplier_for,
    recommend_fixes, recommend_slots, recommend_swap, team_score, team_weakness, weak_spot,
    weakness_of,
};
pub use evolution::{EvolutionNode, chain_of, evolutions};
pub use pokemon::{
    Pokemon, UnknownPokemon, UnknownPokemonId, find, find_by_id, find_team, find_team_by_ids,
    pokedex,
};
pub use types::{Type, effectiveness};
