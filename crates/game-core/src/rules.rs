use serde::{Deserialize, Serialize};

use crate::card::Color;

/// Optional variant rules a room can toggle in the lobby before starting a
/// game. Kept as a small `Copy` struct so it can ride along on `SetRules`,
/// the lobby `Joined` reply, and `GameState`/`PlayerView` itself without any
/// ceremony. `#[serde(default)]` on every field means older clients sending
/// a bare `{}` still deserialize fine as "everything off", which matters
/// once a second rule gets added here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameRules {
    /// Adds a 6th "multicolor" suit (10 cards, same 3/2/2/2/1 distribution
    /// as every other suit). Multicolor cards count as *every* color when
    /// receiving a color clue — so a "Red" clue also touches them — but the
    /// multicolor suit itself can never be clued directly, matching the
    /// standard tabletop variant. It still builds its own separate firework
    /// when played, taking the max possible score from 25 to 30.
    #[serde(default)]
    pub multicolor: bool,
}

impl GameRules {
    /// The colors actually in play for a game using these rules, in stable
    /// display order (multicolor last, since it's the bonus suit). This is
    /// the set both the deck and the fireworks display are built from.
    pub fn active_colors(&self) -> Vec<Color> {
        let mut colors = Color::ALL.to_vec();
        if self.multicolor {
            colors.push(Color::Multicolor);
        }
        colors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_no_variants() {
        assert_eq!(GameRules::default(), GameRules { multicolor: false });
    }

    #[test]
    fn active_colors_adds_multicolor_last_when_enabled() {
        let plain = GameRules::default();
        assert_eq!(plain.active_colors(), Color::ALL.to_vec());

        let with_multi = GameRules { multicolor: true };
        let colors = with_multi.active_colors();
        assert_eq!(colors.len(), 6);
        assert_eq!(colors.last(), Some(&Color::Multicolor));
    }
}
