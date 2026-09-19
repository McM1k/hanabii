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
    /// as every other suit, unless `multicolor_short` is also on — 12 cards,
    /// 3/2/2/2/2/1, if `six_cards` is on instead). Multicolor cards count as
    /// *every* color when receiving a color clue — so a "Red" clue also
    /// touches them — but the multicolor suit itself can never be clued
    /// directly, matching the standard tabletop variant. It still builds
    /// its own separate firework when played, taking the max possible
    /// score up by 5 (6 with `six_cards`).
    #[serde(default)]
    pub multicolor: bool,
    /// Adds a "black powder" suit (10 cards, mirrored 1/2/2/2/3 distribution
    /// — three 5s down to one 1 — unless `black_short` is also on; 12 cards,
    /// mirrored 1/2/2/2/2/3 — three 6s down to one 1 — if `six_cards` is on
    /// instead). Black cards have no color at all for clue purposes: no
    /// color clue, including naming Black directly, ever touches them.
    /// Their firework is also built in *descending* order (5 down to 1, or
    /// 6 down to 1 with `six_cards`) instead of the usual ascending order.
    #[serde(default)]
    pub black: bool,
    /// How many ordinary extra suits to add on top of the standard five —
    /// each behaves exactly like white/red/yellow/green/blue (ascending
    /// 1-5, normal clue matching), just optional. Clamped to 0-2: picked in
    /// priority from colors that aren't white, since another near-white
    /// suit would be easy to confuse with the base white suit and duller
    /// to look at — currently orange (added first) then purple (second).
    #[serde(default)]
    pub extra_colors: u8,
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
    /// Harder variant for whichever extra suits `extra_colors` adds: only
    /// one copy of each rank (5 cards) instead of the usual 3/2/2/2/1 (10
    /// cards). Applies uniformly to all of them; no effect if
    /// `extra_colors` is 0.
    #[serde(default)]
    pub extra_colors_short: bool,
    /// Adds a 6th-rank card to every active suit's distribution — the five
    /// base colors and any optional suits (multicolor, black, orange,
    /// purple) alike. For an ascending suit the previously-unique 5 becomes
    /// an ordinary pair and the new 6 takes over as the unique top card
    /// (`deck::NUMBER_COUNTS_SIX`). For a suit that plays in descending
    /// order (currently just Black) the mirror shifts by one slot instead
    /// of just swapping 5 and 6: 6 becomes the abundant "starting" card
    /// (three copies) and 1 stays the unique "finishing" card, with 5
    /// dropping to an ordinary pair (`deck::REVERSE_NUMBER_COUNTS_SIX`).
    /// Composes with each suit's own "short" option: a short suit becomes
    /// one of every rank 1-6 (6 cards) instead of 1-5 (5 cards). See
    /// `max_rank`/`max_score` for how this feeds into scoring.
    #[serde(default)]
    pub six_cards: bool,
}

/// The extra suits `extra_colors` draws from, in priority order — the
/// first `extra_colors` (clamped to this list's length) of these are
/// added. Kept as one list so `active_colors` and `is_short` can't drift
/// out of sync with each other about which suit is "extra suit #1" vs "#2".
const EXTRA_COLOR_PRIORITY: [Color; 2] = [Color::Orange, Color::Purple];

impl GameRules {
    /// The colors actually in play for a game using these rules, in stable
    /// display order used throughout the app: white, red, orange, yellow,
    /// green, blue, purple, multicolor, black. This is the set both the
    /// deck and the fireworks/discard-pile display are built from.
    pub fn active_colors(&self) -> Vec<Color> {
        let extra_count = (self.extra_colors as usize).min(EXTRA_COLOR_PRIORITY.len());
        let mut colors = Vec::with_capacity(5 + EXTRA_COLOR_PRIORITY.len() + 2);
        colors.push(Color::White);
        colors.push(Color::Red);
        if extra_count >= 1 {
            colors.push(EXTRA_COLOR_PRIORITY[0]);
        }
        colors.push(Color::Yellow);
        colors.push(Color::Green);
        colors.push(Color::Blue);
        if extra_count >= 2 {
            colors.push(EXTRA_COLOR_PRIORITY[1]);
        }
        if self.multicolor {
            colors.push(Color::Multicolor);
        }
        if self.black {
            colors.push(Color::Black);
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
            Color::Orange | Color::Purple => self.extra_colors_short,
            _ => false,
        }
    }

    /// The highest rank any active suit's firework can reach: 6 if the
    /// "6th card" option is on, 5 otherwise. Applies uniformly to every
    /// suit in play — including Black, whose firework just runs in the
    /// other direction (see `is_reverse_suit` in `state.rs`), not to a
    /// different ceiling.
    pub fn max_rank(&self) -> u8 {
        if self.six_cards { 6 } else { 5 }
    }

    /// The score a perfect game would reach: every active suit's firework
    /// built all the way to `max_rank`. Written as a sum over `max_rank`
    /// per suit, rather than `active_colors().len() * max_rank()`, so a
    /// future suit-specific ceiling wouldn't have to touch every caller.
    pub fn max_score(&self) -> u8 {
        self.active_colors().iter().map(|_| self.max_rank()).sum()
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
                extra_colors: 0,
                multicolor_short: false,
                black_short: false,
                extra_colors_short: false,
                six_cards: false,
            }
        );
    }

    #[test]
    fn active_colors_includes_every_enabled_optional_suit() {
        let plain = GameRules::default();
        assert_eq!(plain.active_colors(), Color::ALL.to_vec());

        let all = GameRules {
            multicolor: true,
            black: true,
            extra_colors: 2,
            ..Default::default()
        };
        let colors = all.active_colors();
        assert_eq!(colors.len(), 9);
        assert!(colors.contains(&Color::Multicolor));
        assert!(colors.contains(&Color::Black));
        assert!(colors.contains(&Color::Orange));
        assert!(colors.contains(&Color::Purple));
    }

    #[test]
    fn one_extra_color_adds_only_orange_not_purple() {
        let rules = GameRules { extra_colors: 1, ..Default::default() };
        let colors = rules.active_colors();
        assert_eq!(colors.len(), 6);
        assert!(colors.contains(&Color::Orange));
        assert!(!colors.contains(&Color::Purple));
    }

    #[test]
    fn extra_colors_above_two_is_clamped_to_two() {
        let rules = GameRules { extra_colors: 200, ..Default::default() };
        let colors = rules.active_colors();
        assert_eq!(colors.len(), 7);
        assert!(colors.contains(&Color::Orange));
        assert!(colors.contains(&Color::Purple));
    }

    #[test]
    fn active_colors_follows_the_fixed_display_order() {
        let rules = GameRules {
            multicolor: true,
            black: true,
            extra_colors: 2,
            ..Default::default()
        };
        assert_eq!(
            rules.active_colors(),
            vec![
                Color::White,
                Color::Red,
                Color::Orange,
                Color::Yellow,
                Color::Green,
                Color::Blue,
                Color::Purple,
                Color::Multicolor,
                Color::Black,
            ]
        );
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
    fn extra_colors_short_applies_to_both_extra_suits_uniformly() {
        let rules = GameRules {
            extra_colors: 2,
            extra_colors_short: true,
            ..Default::default()
        };
        assert!(rules.is_short(Color::Orange));
        assert!(rules.is_short(Color::Purple));
    }

    #[test]
    fn max_rank_is_five_unless_six_cards_is_on() {
        assert_eq!(GameRules::default().max_rank(), 5);
        let rules = GameRules { six_cards: true, ..Default::default() };
        assert_eq!(rules.max_rank(), 6);
    }

    #[test]
    fn max_score_is_five_per_active_suit_by_default() {
        let rules = GameRules { multicolor: true, extra_colors: 1, ..Default::default() };
        // 7 active suits (5 base + orange + multicolor) at 5 each.
        assert_eq!(rules.max_score(), 35);
    }

    #[test]
    fn six_cards_raises_max_score_by_one_per_active_suit() {
        let rules = GameRules {
            multicolor: true,
            extra_colors: 1,
            six_cards: true,
            ..Default::default()
        };
        // Same 7 active suits, now at 6 each.
        assert_eq!(rules.max_score(), 42);
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
