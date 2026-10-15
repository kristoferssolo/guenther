use crate::bingo::{error::BingoError, model::GameId};
use strum::{AsRefStr, Display, EnumString};
use teloxide::types::ChatId;

pub const MAX_GAME_DESCRIPTION_CHARS: usize = 256;

/// Lifecycle of a game; the lowercase names are stored in `bingo_games.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, Display, EnumString)]
#[strum(
    serialize_all = "lowercase",
    parse_err_ty = BingoError,
    parse_err_fn = invalid_stored_game_state
)]
pub enum GameState {
    Draft,
    Active,
    Closed,
}

fn invalid_stored_game_state(value: &str) -> BingoError {
    BingoError::InvalidStoredGameState(value.to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: GameId,
    pub chat_id: ChatId,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub center_text: String,
    pub state: GameState,
    pub is_default: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok_eq;
    use rstest::rstest;
    use std::assert_matches;

    #[rstest]
    #[case(GameState::Draft, "draft")]
    #[case(GameState::Active, "active")]
    #[case(GameState::Closed, "closed")]
    fn game_state_round_trips_stored_names(#[case] state: GameState, #[case] name: &str) {
        assert_eq!(state.as_ref(), name);
        assert_ok_eq!(name.parse::<GameState>(), state);
    }

    #[test]
    fn game_state_parse_is_case_sensitive() {
        assert_matches!(
            "Active".parse::<GameState>(),
            Err(BingoError::InvalidStoredGameState(value)) if value == "Active"
        );
    }
}
