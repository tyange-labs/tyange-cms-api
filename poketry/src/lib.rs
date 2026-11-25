pub mod analysis;
pub mod evolution;
pub mod pokemon;
pub mod types;

pub use analysis::{
    Recommendation, Swap, Weakness, defense_multiplier, recommend, recommend_swap, team_score,
    team_weakness, weak_spot,
};
pub use pokemon::{Pokemon, UnknownPokemon, find, find_team, pokedex};
pub use types::{Type, effectiveness};
