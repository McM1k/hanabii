use serde::{Deserialize, Serialize};

use crate::card::Color;

/// Optional variant rules a room can toggle in the lobby before starting a
/// game. Kept as a small `Copy` struct so it can ride along on `SetRules`,
/// the lobby `Joined` reply, and `GameState`/`PlayerView` itself without any
/// ceremony. `#[serde(default)]` on every field means older clients sending
/// a partial (or bare `{}`) payload still deserialize fine, with anything
/// missing defaulting to off.
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
    /// Adds a "black powder" suit (10 cards, mirrored 1/2/2/2/3
    /// distribution — three 5s down to one 1). Black cards have no color
    /// at all for clue purposes: no color clue, including naming Black
    /// directly, ever touches them. Their firework is also built in
    /// *descending* order, 5 down to 1, instead of the usual 1 to 5.
    /// Independent of `multicolor` — either, both, or neither can be on.
    #[serde(default)]
    pub black: bool,
}

impl GameRules {
    /// The colors actually in play for a game using these rules, in stable
    /// display order (the optional suits last, since they're the "bonus"
    /// ones). This is the set both the deck and the fireworks display are
    /// built from.
    pub fn active_colors(&self) -> Vec<Color> {
        let mut colors = Color::ALL.to_vec();
        if self.multicolor {
            colors.push(Color::Multicolor);
        }
        if self.black {
            colors.push(Color::Black);
        }
        colors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_no_variants() {
        assert_eq!(
            GameRules::default(),
            GameRules { multicolor: false, black: false }
        );
    }

    #[test]
    fn active_colors_adds_multicolor_last_when_enabled() {
        let plain = GameRules::default();
        assert_eq!(plain.active_colors(), Color::ALL.to_vec());

        let with_multi = GameRules { multicolor: true, black: false };
        let colors = with_multi.active_colors();
        assert_eq!(colors.len(), 6);
        assert_eq!(colors.last(), Some(&Color::Multicolor));
    }

    #[test]
    fn active_colors_adds_black_last_when_enabled() {
        let with_black = GameRules { multicolor: false, black: true };
        let colors = with_black.active_colors();
        assert_eq!(colors.len(), 6);
        assert_eq!(colors.last(), Some(&Color::Black));
    }

    #[test]
    fn both_variants_can_be_on_at_once() {
        let both = GameRules { multicolor: true, black: true };
        let colors = both.active_colors();
        assert_eq!(colors.len(), 7);
        assert!(colors.contains(&Color::Multicolor));
        assert!(colors.contains(&Color::Black));
    }
}
