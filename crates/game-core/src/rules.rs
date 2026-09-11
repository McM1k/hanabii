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
    /// as every other suit, unless `multicolor_short` is also on). Multicolor
    /// cards count as *every* color when receiving a color clue — so a "Red"
    /// clue also touches them — but the multicolor suit itself can never be
    /// clued directly, matching the standard tabletop variant. It still
    /// builds its own separate firework when played, taking the max
    /// possible score up by 5.
    #[serde(default)]
    pub multicolor: bool,
    /// Adds a "black powder" suit (10 cards, mirrored 1/2/2/2/3 distribution
    /// — three 5s down to one 1 — unless `black_short` is also on). Black
    /// cards have no color at all for clue purposes: no color clue,
    /// including naming Black directly, ever touches them. Their firework
    /// is also built in *descending* order, 5 down to 1, instead of the
    /// usual 1 to 5.
    #[serde(default)]
    pub black: bool,
    /// Adds a plain "orange" suit — behaves exactly like the five base
    /// colors (ascending 1-5, normal clue matching), just optional.
    #[serde(default)]
    pub orange: bool,
    /// Adds a plain "purple" suit — behaves exactly like the five base
    /// colors (ascending 1-5, normal clue matching), just optional.
    #[serde(default)]
    pub purple: bool,
    /// Harder variant of the multicolor suit: only one copy of each rank
    /// (5 cards total) instead of the usual 3/2/2/2/1 (10 cards) — every
    /// card becomes irreplaceable. No effect unless `multicolor` is also on.
    #[serde(default)]
    pub multicolor_short: bool,
    /// Harder variant of the black suit: only one copy of each rank (5
    /// cards total) instead of the usual mirrored 1/2/2/2/3 (10 cards). No
    /// effect unless `black` is also on.
    #[serde(default)]
    pub black_short: bool,
    /// Harder variant of the orange suit: only one copy of each rank (5
    /// cards total) instead of the usual 3/2/2/2/1 (10 cards). No effect
    /// unless `orange` is also on.
    #[serde(default)]
    pub orange_short: bool,
    /// Harder variant of the purple suit: only one copy of each rank (5
    /// cards total) instead of the usual 3/2/2/2/1 (10 cards). No effect
    /// unless `purple` is also on.
    #[serde(default)]
    pub purple_short: bool,
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
        if self.orange {
            colors.push(Color::Orange);
        }
        if self.purple {
            colors.push(Color::Purple);
        }
        colors
    }

    /// Whether the given suit uses the "short" one-copy-of-each-rank
    /// distribution instead of its normal one. Always false for the five
    /// base colors, which don't have a short option, and doesn't itself
    /// check whether the suit is active — it's `active_colors` that decides
    /// whether a suit (and so its distribution) matters at all.
    pub fn is_short(&self, color: Color) -> bool {
        match color {
            Color::Multicolor => self.multicolor_short,
            Color::Black => self.black_short,
            Color::Orange => self.orange_short,
            Color::Purple => self.purple_short,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_no_variants() {
        assert_eq!(
            GameRules::default(),
            GameRules {
                multicolor: false,
                black: false,
                orange: false,
                purple: false,
                multicolor_short: false,
                black_short: false,
                orange_short: false,
                purple_short: false,
            }
        );
    }

    #[test]
    fn active_colors_includes_every_enabled_optional_suit() {
        let plain = GameRules::default();
        assert_eq!(plain.active_colors(), Color::ALL.to_vec());

        let all_four = GameRules {
            multicolor: true,
            black: true,
            orange: true,
            purple: true,
            ..Default::default()
        };
        let colors = all_four.active_colors();
        assert_eq!(colors.len(), 9);
        assert!(colors.contains(&Color::Multicolor));
        assert!(colors.contains(&Color::Black));
        assert!(colors.contains(&Color::Orange));
        assert!(colors.contains(&Color::Purple));
    }

    #[test]
    fn short_flags_only_apply_to_their_own_suit() {
        let rules = GameRules {
            multicolor: true,
            multicolor_short: true,
            black: true,
            // black_short deliberately left off
            ..Default::default()
        };
        assert!(rules.is_short(Color::Multicolor));
        assert!(!rules.is_short(Color::Black));
        // Base colors never have a short option, regardless of any flag —
        // there isn't one to set, but the method stays total.
        assert!(!rules.is_short(Color::Red));
    }

    #[test]
    fn short_flag_alone_does_not_activate_a_suit() {
        let rules = GameRules {
            multicolor: false,
            multicolor_short: true,
            ..Default::default()
        };
        // is_short reports the flag regardless — it's active_colors that
        // decides whether a suit (and so its distribution) is in the deck
        // at all.
        assert!(rules.is_short(Color::Multicolor));
        assert!(!rules.active_colors().contains(&Color::Multicolor));
    }
}
