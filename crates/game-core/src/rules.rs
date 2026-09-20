use serde::{Deserialize, Serialize};

use crate::card::{Card, Clue, Color};

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
    /// How many extra suits to bring in beyond red/yellow/green/blue —
    /// each behaves exactly like any base suit (ascending 1-5, normal clue
    /// matching), just optional. Clamped to 0-2, and this isn't "0, 1, or 2
    /// extra suits added to the same base five" — White itself moves:
    /// - 0: just the plain five (white/red/yellow/green/blue).
    /// - 1: Orange and Purple *both* come in, and White drops out to make
    ///   room — White is already the base suit easiest to mistake for "no
    ///   clue yet" (a blank card reads a lot like a white one), so it's the
    ///   one that goes when there's a pair of new suits to make space for.
    ///   Net effect: 6 suits (red/orange/yellow/green/blue/purple).
    /// - 2: White comes back on top of that — all seven suits at once
    ///   (white/red/orange/yellow/green/blue/purple).
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
    /// The "hanabii" game mode (two i's — the real deal): a fixed preset
    /// that *replaces* every other option here rather than combining with
    /// them. See [`GameRules::normalized`] for exactly what it forces.
    ///
    /// It plays like a game with six colors (red, orange, yellow, green,
    /// blue, purple) and six-card suits (3/2/2/2/2/1 of ranks 1-6 per
    /// color, 72 cards, max score 36) — plus its own color-clue rule:
    /// - Only the three primary colors (red, yellow, blue) can be named in
    ///   a color clue.
    /// - Every other color is mixed from primaries — orange is red +
    ///   yellow, green is yellow + blue, purple is red + blue — and a card
    ///   is touched by a primary clue when that primary is one of its
    ///   ingredients. So a red clue touches red, orange *and* purple cards.
    ///
    /// Number clues are the same as ever. See [`GameRules::cluable_colors`]
    /// and [`GameRules::color_clue_touches`] for the clue rules themselves.
    #[serde(default)]
    pub hanabii: bool,
}

impl GameRules {
    /// The rules a game is actually played with. For ordinary rules that's
    /// just `self` untouched; with [`GameRules::hanabii`] on, every other
    /// option is overridden by the mode's fixed preset — which is what
    /// "picking hanabii locks the other options" means in the engine:
    /// - `extra_colors: 1` (Orange and Purple in, White out — see that
    ///   field's doc comment), giving red/orange/yellow/green/blue/purple,
    /// - `six_cards: true` (3/2/2/2/2/1 of ranks 1-6 in every color),
    /// - no multicolor or black suit, and no "short" (one-of-each) suits.
    ///
    /// Idempotent, and the server applies it whenever the lobby rules
    /// change (so every client sees the locked-in preset) as well as when
    /// a game is created, so a client can't sneak extra options in next to
    /// the mode by sending a hand-built `SetRules`. [`GameRules::active_colors`],
    /// [`GameRules::is_short`] and [`GameRules::max_rank`] all read
    /// through it too, so they answer for the mode even on un-normalized
    /// rules.
    pub fn normalized(&self) -> GameRules {
        if self.hanabii {
            GameRules {
                hanabii: true,
                extra_colors: 1,
                six_cards: true,
                ..GameRules::default()
            }
        } else {
            *self
        }
    }

    /// The colors actually in play for a game using these rules, in stable
    /// display order used throughout the app: white, red, orange, yellow,
    /// green, blue, purple, multicolor, black — except White drops out
    /// entirely at `extra_colors == 1` (see its doc comment). This is the
    /// set both the deck and the fireworks/discard-pile display are built
    /// from.
    pub fn active_colors(&self) -> Vec<Color> {
        let rules = self.normalized();
        let extra_level = rules.extra_colors.min(2);
        let mut colors = Vec::with_capacity(7 + 2);
        // White sits out only at exactly 1 — both Orange and Purple come
        // in together there to make room for it, and it's back the moment
        // the count reaches 2 (see `extra_colors`'s doc comment for why).
        if extra_level != 1 {
            colors.push(Color::White);
        }
        colors.push(Color::Red);
        if extra_level >= 1 {
            colors.push(Color::Orange);
        }
        colors.push(Color::Yellow);
        colors.push(Color::Green);
        colors.push(Color::Blue);
        if extra_level >= 1 {
            colors.push(Color::Purple);
        }
        if rules.multicolor {
            colors.push(Color::Multicolor);
        }
        if rules.black {
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
        let rules = self.normalized();
        match color {
            Color::Multicolor => rules.multicolor_short,
            Color::Black => rules.black_short,
            Color::Orange | Color::Purple => rules.extra_colors_short,
            _ => false,
        }
    }

    /// The highest rank any active suit's firework can reach: 6 if the
    /// "6th card" option is on (which the hanabii mode always is), 5
    /// otherwise. Applies uniformly to every suit in play — including
    /// Black, whose firework just runs in the other direction (see
    /// `is_reverse_suit` in `state.rs`), not to a different ceiling.
    pub fn max_rank(&self) -> u8 {
        if self.normalized().six_cards { 6 } else { 5 }
    }

    /// The score a perfect game would reach: every active suit's firework
    /// built all the way to `max_rank`. Written as a sum over `max_rank`
    /// per suit, rather than `active_colors().len() * max_rank()`, so a
    /// future suit-specific ceiling wouldn't have to touch every caller.
    pub fn max_score(&self) -> u8 {
        self.active_colors().iter().map(|_| self.max_rank()).sum()
    }

    /// The colors a player is allowed to name in a color clue. Ordinarily
    /// that's every active color except Multicolor (wild when *receiving*
    /// a clue, but never the color named in one) and Black (no color at
    /// all). In hanabii mode it's just the three primaries — red, yellow
    /// and blue — regardless of which colors are in the deck.
    ///
    /// This is the list a UI should offer; the engine enforces it in
    /// `GameState::apply_clue` with its own dedicated errors.
    pub fn cluable_colors(&self) -> Vec<Color> {
        if self.hanabii {
            Color::PRIMARIES.to_vec()
        } else {
            self.active_colors()
                .into_iter()
                .filter(|&color| color != Color::Multicolor && color != Color::Black)
                .collect()
        }
    }

    /// Whether a color clue naming `clue` touches a card of color
    /// `card_color` — the single definition of color-clue matching, shared
    /// by the engine and by the frontend's hover preview so the two can't
    /// disagree.
    ///
    /// Ordinarily a card is touched by its own color, and by *any* color
    /// clue if it's Multicolor (Black, having no color, is never touched).
    /// In hanabii mode a card is touched when the named primary is one of
    /// its ingredients ([`Color::primary_components`]): red touches red,
    /// orange and purple; yellow touches yellow, orange and green; blue
    /// touches blue, green and purple.
    pub fn color_clue_touches(&self, clue: Color, card_color: Color) -> bool {
        if self.hanabii {
            card_color.primary_components().contains(&clue)
        } else {
            card_color == clue || card_color == Color::Multicolor
        }
    }

    /// Whether `clue` (a color or a number) touches `card` under these
    /// rules. Number clues are the same in every mode.
    pub fn clue_touches(&self, clue: Clue, card: Card) -> bool {
        match clue {
            Clue::Color(color) => self.color_clue_touches(color, card.color),
            Clue::Number(number) => card.number == number,
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
                extra_colors: 0,
                multicolor_short: false,
                black_short: false,
                extra_colors_short: false,
                six_cards: false,
                hanabii: false,
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
    fn one_extra_color_brings_in_both_orange_and_purple_and_drops_white() {
        let rules = GameRules { extra_colors: 1, ..Default::default() };
        let colors = rules.active_colors();
        // One more than the plain game — White is out, but both Orange
        // and Purple are in.
        assert_eq!(colors.len(), 6);
        assert!(colors.contains(&Color::Orange));
        assert!(colors.contains(&Color::Purple));
        assert!(!colors.contains(&Color::White));
    }

    #[test]
    fn two_extra_colors_keeps_white_and_adds_orange_and_purple() {
        let rules = GameRules { extra_colors: 2, ..Default::default() };
        let colors = rules.active_colors();
        assert_eq!(colors.len(), 7);
        assert!(colors.contains(&Color::White));
        assert!(colors.contains(&Color::Orange));
        assert!(colors.contains(&Color::Purple));
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
    fn one_extra_color_display_order_has_no_white() {
        let rules = GameRules { extra_colors: 1, ..Default::default() };
        assert_eq!(
            rules.active_colors(),
            vec![
                Color::Red,
                Color::Orange,
                Color::Yellow,
                Color::Green,
                Color::Blue,
                Color::Purple,
            ],
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
        // 7 active suits (red/orange/yellow/green/blue/purple — no white
        // at extra_colors: 1 — plus multicolor) at 5 each.
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

    // --- hanabii mode -----------------------------------------------------

    fn hanabii() -> GameRules {
        GameRules { hanabii: true, ..Default::default() }
    }

    #[test]
    fn hanabii_normalizes_to_a_fixed_six_color_six_card_preset() {
        assert_eq!(
            hanabii().normalized(),
            GameRules {
                hanabii: true,
                extra_colors: 1,
                six_cards: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn hanabii_overrides_every_other_option() {
        // Every other toggle switched on, and the mode still comes out as
        // the exact same preset — nothing leaks through next to it.
        let everything = GameRules {
            multicolor: true,
            black: true,
            extra_colors: 2,
            multicolor_short: true,
            black_short: true,
            extra_colors_short: true,
            six_cards: false,
            hanabii: true,
        };
        assert_eq!(everything.normalized(), hanabii().normalized());
    }

    #[test]
    fn normalization_is_idempotent() {
        let once = hanabii().normalized();
        assert_eq!(once.normalized(), once);
    }

    #[test]
    fn normalized_leaves_ordinary_rules_untouched() {
        let rules = GameRules {
            multicolor: true,
            black_short: true,
            extra_colors: 2,
            six_cards: true,
            ..Default::default()
        };
        assert_eq!(rules.normalized(), rules);
        assert_eq!(GameRules::default().normalized(), GameRules::default());
    }

    #[test]
    fn hanabii_plays_with_red_orange_yellow_green_blue_purple_in_that_order() {
        let expected = vec![
            Color::Red,
            Color::Orange,
            Color::Yellow,
            Color::Green,
            Color::Blue,
            Color::Purple,
        ];
        // Same answer whether or not the caller normalized first.
        assert_eq!(hanabii().active_colors(), expected);
        assert_eq!(hanabii().normalized().active_colors(), expected);
        // ...and no White, Multicolor or Black even if asked for.
        let greedy = GameRules { multicolor: true, black: true, hanabii: true, ..Default::default() };
        assert_eq!(greedy.active_colors(), expected);
    }

    #[test]
    fn hanabii_has_six_ranks_no_short_suits_and_a_max_score_of_36() {
        let rules = GameRules { multicolor_short: true, extra_colors_short: true, hanabii: true, ..Default::default() };
        assert_eq!(rules.max_rank(), 6);
        assert_eq!(rules.max_score(), 36);
        for color in rules.active_colors() {
            assert!(!rules.is_short(color), "{color:?} shouldn't be short in hanabii mode");
        }
    }

    #[test]
    fn hanabii_only_offers_the_primary_colors_to_clue() {
        assert_eq!(hanabii().cluable_colors(), vec![Color::Red, Color::Yellow, Color::Blue]);
        assert_eq!(hanabii().normalized().cluable_colors(), vec![Color::Red, Color::Yellow, Color::Blue]);
    }

    #[test]
    fn ordinary_games_offer_every_active_color_but_multicolor_and_black() {
        assert_eq!(GameRules::default().cluable_colors(), Color::ALL.to_vec());
        let rules = GameRules { multicolor: true, black: true, extra_colors: 2, ..Default::default() };
        assert_eq!(
            rules.cluable_colors(),
            vec![
                Color::White,
                Color::Red,
                Color::Orange,
                Color::Yellow,
                Color::Green,
                Color::Blue,
                Color::Purple,
            ]
        );
    }

    #[test]
    fn hanabii_color_clues_touch_every_color_mixed_with_the_named_primary() {
        let rules = hanabii();
        let touched_by = |primary: Color| -> Vec<Color> {
            rules
                .active_colors()
                .into_iter()
                .filter(|&c| rules.color_clue_touches(primary, c))
                .collect()
        };
        // The example straight from the mode's definition: a red clue hits
        // red, and also purple (blue + red) and orange (red + yellow).
        assert_eq!(touched_by(Color::Red), vec![Color::Red, Color::Orange, Color::Purple]);
        assert_eq!(touched_by(Color::Yellow), vec![Color::Orange, Color::Yellow, Color::Green]);
        assert_eq!(touched_by(Color::Blue), vec![Color::Green, Color::Blue, Color::Purple]);
    }

    #[test]
    fn every_hanabii_color_is_touched_by_exactly_its_own_ingredients() {
        // The same table read the other way round: how many of the three
        // primary clues would touch each color.
        let rules = hanabii();
        for (color, expected) in [
            (Color::Red, 1),
            (Color::Yellow, 1),
            (Color::Blue, 1),
            (Color::Orange, 2),
            (Color::Green, 2),
            (Color::Purple, 2),
        ] {
            let touching = Color::PRIMARIES
                .iter()
                .filter(|&&p| rules.color_clue_touches(p, color))
                .count();
            assert_eq!(touching, expected, "{color:?}");
        }
    }

    #[test]
    fn ordinary_color_clues_touch_their_own_color_and_multicolor_only() {
        let rules = GameRules { multicolor: true, black: true, ..Default::default() };
        assert!(rules.color_clue_touches(Color::Red, Color::Red));
        assert!(rules.color_clue_touches(Color::Red, Color::Multicolor));
        assert!(!rules.color_clue_touches(Color::Red, Color::Blue));
        assert!(!rules.color_clue_touches(Color::Red, Color::Black));
        // Notably, in an ordinary game red doesn't touch orange or purple
        // — that's what's special about hanabii mode.
        assert!(!rules.color_clue_touches(Color::Red, Color::Orange));
        assert!(!rules.color_clue_touches(Color::Red, Color::Purple));
    }

    #[test]
    fn number_clues_touch_by_rank_in_every_mode() {
        let card = Card { color: Color::Orange, number: 3 };
        for rules in [GameRules::default(), hanabii()] {
            assert!(rules.clue_touches(Clue::Number(3), card));
            assert!(!rules.clue_touches(Clue::Number(4), card));
        }
        assert!(hanabii().clue_touches(Clue::Color(Color::Red), card));
        assert!(!GameRules::default().clue_touches(Clue::Color(Color::Red), card));
    }
}
