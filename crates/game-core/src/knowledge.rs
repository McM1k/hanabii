use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::card::{Clue, Color, Number};
use crate::rules::GameRules;

/// Everything a player has been told about one of their own cards, built up
/// clue by clue. Positive info comes from being directly clued; negative
/// info comes from *not* being touched by a clue given to the rest of the
/// hand (e.g. "these two are red" also tells you your other cards aren't
/// red) — standard Hanabi etiquette assumes players track this.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardKnowledge {
    pub known_color: Option<Color>,
    pub known_number: Option<Number>,
    pub not_colors: HashSet<Color>,
    pub not_numbers: HashSet<Number>,
    /// Every *distinct* color a color clue has positively touched this card
    /// with. Almost always has at most one entry (a card only ever matches
    /// clues of its own color) — but see `inferred_multicolor`.
    pub clued_colors: HashSet<Color>,
    /// Hanabii mode only (see [`GameRules::hanabii`]): the primary colors
    /// whose clue has *touched* this card. Kept apart from `clued_colors` /
    /// `known_color` on purpose — in that mode a card matching two
    /// different colors is perfectly normal (a red-and-yellow card is just
    /// orange), so the ordinary "two colors means multicolor" reading of
    /// `clued_colors` would be plain wrong, and a single red hit doesn't
    /// mean the card *is* red. Empty in every other mode.
    #[serde(default)]
    pub hit_primaries: HashSet<Color>,
    /// Hanabii mode only: the primary colors whose clue has been given to
    /// this card's hand *without* touching it. Empty in every other mode.
    #[serde(default)]
    pub missed_primaries: HashSet<Color>,
}

impl CardKnowledge {
    pub fn apply_positive(&mut self, clue: Clue) {
        match clue {
            Clue::Color(c) => {
                self.known_color = Some(c);
                self.clued_colors.insert(c);
            }
            Clue::Number(n) => self.known_number = Some(n),
        }
    }

    pub fn apply_negative(&mut self, clue: Clue) {
        match clue {
            Clue::Color(c) => {
                self.not_colors.insert(c);
            }
            Clue::Number(n) => {
                self.not_numbers.insert(n);
            }
        }
    }

    /// Records what one clue did to this card — `touched` is whether it
    /// matched — under the game's rules. This is what the engine calls;
    /// [`CardKnowledge::apply_positive`] / [`CardKnowledge::apply_negative`]
    /// remain the ordinary-game primitives it falls back to.
    ///
    /// The only difference is a color clue in hanabii mode: the result is
    /// kept as evidence about *primary colors* (`hit_primaries` /
    /// `missed_primaries`) instead of being turned into a claim about the
    /// card's own color, because a red hit means "red, orange or purple",
    /// not "red". See `hanabii_possible_colors` for what that evidence
    /// works out to.
    pub fn apply_clue_result(&mut self, clue: Clue, touched: bool, rules: &GameRules) {
        match clue {
            Clue::Color(primary) if rules.hanabii => {
                if touched {
                    self.hit_primaries.insert(primary);
                } else {
                    self.missed_primaries.insert(primary);
                }
            }
            _ if touched => self.apply_positive(clue),
            _ => self.apply_negative(clue),
        }
    }

    /// Hanabii mode: whether a card of `color` could have produced every
    /// primary-color result recorded so far. A color is ruled out as soon
    /// as it contradicts a single result — it's missing a primary that
    /// touched the card, or it contains one that didn't:
    /// - a red hit rules out yellow, green and blue (none contain red),
    /// - a red miss rules out red, orange and purple (all contain red).
    ///
    /// A hard deduction from the clue history, like `inferred_black`.
    pub fn could_be_hanabii_color(&self, color: Color) -> bool {
        let ingredients = color.primary_components();
        self.hit_primaries.iter().all(|p| ingredients.contains(p))
            && self.missed_primaries.iter().all(|p| !ingredients.contains(p))
    }

    /// Hanabii mode: the colors in play this card could still be, in the
    /// game's display order. All of them until a color clue has landed;
    /// a single one once the card's color is pinned down (for example by
    /// hits from two different primaries, or misses from two of them).
    /// Only meaningful when `rules.hanabii` is on.
    pub fn hanabii_possible_colors(&self, rules: &GameRules) -> Vec<Color> {
        rules
            .active_colors()
            .into_iter()
            .filter(|&color| self.could_be_hanabii_color(color))
            .collect()
    }

    /// Hanabii mode: the card's color, once the clues so far leave only
    /// one possibility. Unlike `known_color` (which just remembers the last
    /// color clued) this is never a guess.
    pub fn hanabii_certain_color(&self, rules: &GameRules) -> Option<Color> {
        match self.hanabii_possible_colors(rules).as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// The colors in play this card is provably *not*, in display order —
    /// what the own-hand "ruled out" marks show. Ordinarily that's just the
    /// colors a clue has missed (`not_colors`); in hanabii mode it's
    /// everything the primary-color evidence contradicts (see
    /// `could_be_hanabii_color`), so a single miss on red strikes red,
    /// orange *and* purple at once.
    pub fn ruled_out_colors(&self, rules: &GameRules) -> Vec<Color> {
        rules
            .active_colors()
            .into_iter()
            .filter(|color| {
                if rules.hanabii {
                    !self.could_be_hanabii_color(*color)
                } else {
                    self.not_colors.contains(color)
                }
            })
            .collect()
    }

    /// True once two or more *different* color clues have touched this
    /// card. A real single-colored card can only ever match clues of its
    /// own color, so matching two different ones is only possible for the
    /// multicolor suit — this is a hard deduction from the clue history,
    /// not a guess, and holds however the card knowledge was assembled.
    /// (Never fires in hanabii mode, where that evidence lives in
    /// `hit_primaries` instead — there, two different hits just mean an
    /// orange/green/purple card.)
    pub fn inferred_multicolor(&self) -> bool {
        self.clued_colors.len() > 1
    }

    /// True once every *other* color actually in play for this game has
    /// been ruled out by a negative color clue (given to the rest of the
    /// hand, never touching this card). Needs `rules` rather than just
    /// assuming the five base colors, since orange and purple are ordinary
    /// cluable colors too when they're turned on — ruling out only the
    /// base five wouldn't actually eliminate them as possibilities. This
    /// also rules out multicolor along the way for free: a multicolor card
    /// is touched by *every* color clue, so it can never accumulate even
    /// one negative color result, let alone all of them — meaning the only
    /// suit left once everything else is ruled out is black. Like
    /// `inferred_multicolor`, this is a hard deduction, not a guess.
    pub fn inferred_black(&self, rules: &GameRules) -> bool {
        rules
            .active_colors()
            .into_iter()
            .filter(|&c| c != Color::Black && c != Color::Multicolor)
            .all(|c| self.not_colors.contains(&c))
    }

    /// True if this card could still plausibly be the multicolor wildcard,
    /// given what's been clued so far. Requires: the multicolor suit is
    /// actually in this game, exactly one color has matched so far (two
    /// different ones would already be `inferred_multicolor` — a certainty,
    /// not a maybe), and — the part that's easy to miss — no color clue has
    /// ever come back *negative* on this card. A multicolor card matches
    /// every color clue unconditionally, so even a single miss on some
    /// other color proves it isn't multicolor, no matter how many clues
    /// matched earlier.
    pub fn could_be_multicolor(&self, rules: &GameRules) -> bool {
        rules.multicolor
            && self.known_color.is_some()
            && !self.inferred_multicolor()
            && self.not_colors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_clue_sets_known_value() {
        let mut k = CardKnowledge::default();
        k.apply_positive(Clue::Color(Color::Red));
        assert_eq!(k.known_color, Some(Color::Red));
        assert_eq!(k.known_number, None);
    }

    #[test]
    fn negative_clue_accumulates() {
        let mut k = CardKnowledge::default();
        k.apply_negative(Clue::Number(1));
        k.apply_negative(Clue::Number(2));
        assert!(k.not_numbers.contains(&1));
        assert!(k.not_numbers.contains(&2));
        assert!(!k.not_numbers.contains(&3));
    }

    #[test]
    fn two_different_color_clues_imply_multicolor() {
        let mut k = CardKnowledge::default();
        k.apply_positive(Clue::Color(Color::Red));
        assert!(!k.inferred_multicolor());
        k.apply_positive(Clue::Color(Color::Blue));
        assert!(k.inferred_multicolor());
        // known_color tracks the most recent clue regardless.
        assert_eq!(k.known_color, Some(Color::Blue));
    }

    #[test]
    fn repeating_the_same_color_clue_does_not_imply_multicolor() {
        let mut k = CardKnowledge::default();
        k.apply_positive(Clue::Color(Color::Red));
        k.apply_positive(Clue::Color(Color::Red));
        assert!(!k.inferred_multicolor());
    }

    #[test]
    fn ruling_out_every_base_color_implies_black() {
        let rules = GameRules { black: true, ..Default::default() };
        let mut k = CardKnowledge::default();
        for color in [Color::White, Color::Red, Color::Yellow, Color::Green] {
            k.apply_negative(Clue::Color(color));
            assert!(!k.inferred_black(&rules), "shouldn't be certain before all five are ruled out");
        }
        k.apply_negative(Clue::Color(Color::Blue));
        assert!(k.inferred_black(&rules));
    }

    #[test]
    fn a_single_positive_color_clue_rules_out_black_forever() {
        let rules = GameRules { black: true, ..Default::default() };
        let mut k = CardKnowledge::default();
        for color in [Color::White, Color::Red, Color::Yellow, Color::Green] {
            k.apply_negative(Clue::Color(color));
        }
        // Matched Blue instead of missing it — can't be black after all.
        k.apply_positive(Clue::Color(Color::Blue));
        assert!(!k.inferred_black(&rules));
    }

    #[test]
    fn every_active_extra_suit_must_be_ruled_out_before_inferring_black() {
        // Both Orange and Purple are on (via extra_colors: 1) alongside
        // black in this game — and White has dropped out to make room for
        // them, see `GameRules::extra_colors` — so ruling out just
        // red/yellow/green/blue isn't enough: both extra suits have to be
        // eliminated too, not just one of them.
        let rules = GameRules { black: true, extra_colors: 1, ..Default::default() };
        let mut k = CardKnowledge::default();
        for color in [Color::Red, Color::Yellow, Color::Green, Color::Blue] {
            k.apply_negative(Clue::Color(color));
        }
        assert!(!k.inferred_black(&rules), "orange and purple haven't been ruled out yet");

        k.apply_negative(Clue::Color(Color::Orange));
        assert!(!k.inferred_black(&rules), "purple hasn't been ruled out yet");

        k.apply_negative(Clue::Color(Color::Purple));
        assert!(k.inferred_black(&rules));
    }

    #[test]
    fn a_single_color_match_could_still_be_multicolor() {
        let rules = GameRules { multicolor: true, ..Default::default() };
        let mut k = CardKnowledge::default();
        assert!(!k.could_be_multicolor(&rules), "nothing clued yet");

        k.apply_positive(Clue::Color(Color::Red));
        assert!(k.could_be_multicolor(&rules));
    }

    #[test]
    fn a_later_negative_color_clue_rules_out_multicolor() {
        // Exactly the scenario a player would hit in a real game: clued
        // Red (matched), then Blue is clued to the rest of the hand and
        // this card is *not* touched. A multicolor card would have to
        // match every color clue, so missing this one proves it can't be
        // multicolor after all — even though only one color has ever
        // matched.
        let rules = GameRules { multicolor: true, ..Default::default() };
        let mut k = CardKnowledge::default();
        k.apply_positive(Clue::Color(Color::Red));
        assert!(k.could_be_multicolor(&rules));

        k.apply_negative(Clue::Color(Color::Blue));
        assert!(!k.could_be_multicolor(&rules));
        // The known color itself is untouched by this.
        assert_eq!(k.known_color, Some(Color::Red));
    }

    #[test]
    fn matching_a_second_different_color_is_certainty_not_ambiguity() {
        // Once inferred_multicolor fires, could_be_multicolor should no
        // longer claim it's just a maybe — it's a known fact at that point.
        let rules = GameRules { multicolor: true, ..Default::default() };
        let mut k = CardKnowledge::default();
        k.apply_positive(Clue::Color(Color::Red));
        k.apply_positive(Clue::Color(Color::Blue));
        assert!(k.inferred_multicolor());
        assert!(!k.could_be_multicolor(&rules));
    }

    #[test]
    fn could_be_multicolor_is_false_when_the_rule_is_off() {
        let rules = GameRules::default();
        let mut k = CardKnowledge::default();
        k.apply_positive(Clue::Color(Color::Red));
        assert!(!k.could_be_multicolor(&rules));
    }

    // --- hanabii mode -----------------------------------------------------

    fn hanabii_rules() -> GameRules {
        GameRules { hanabii: true, ..Default::default() }
    }

    fn hit(k: &mut CardKnowledge, primary: Color) {
        k.apply_clue_result(Clue::Color(primary), true, &hanabii_rules());
    }

    fn miss(k: &mut CardKnowledge, primary: Color) {
        k.apply_clue_result(Clue::Color(primary), false, &hanabii_rules());
    }

    #[test]
    fn a_red_hit_leaves_red_orange_and_purple_possible() {
        let rules = hanabii_rules();
        let mut k = CardKnowledge::default();
        assert_eq!(k.hanabii_possible_colors(&rules).len(), 6, "no clue yet: anything goes");
        assert!(k.ruled_out_colors(&rules).is_empty());

        hit(&mut k, Color::Red);
        assert_eq!(
            k.hanabii_possible_colors(&rules),
            vec![Color::Red, Color::Orange, Color::Purple]
        );
        assert_eq!(
            k.ruled_out_colors(&rules),
            vec![Color::Yellow, Color::Green, Color::Blue]
        );
        assert_eq!(k.hanabii_certain_color(&rules), None);
        // Crucially, a red hit doesn't claim the card *is* red.
        assert_eq!(k.known_color, None);
        assert!(k.hit_primaries.contains(&Color::Red));
    }

    #[test]
    fn a_red_miss_rules_out_red_orange_and_purple() {
        let rules = hanabii_rules();
        let mut k = CardKnowledge::default();
        miss(&mut k, Color::Red);
        assert_eq!(
            k.ruled_out_colors(&rules),
            vec![Color::Red, Color::Orange, Color::Purple]
        );
        assert_eq!(
            k.hanabii_possible_colors(&rules),
            vec![Color::Yellow, Color::Green, Color::Blue]
        );
        assert!(k.missed_primaries.contains(&Color::Red));
        // The ordinary negative-clue bookkeeping is left alone in this mode.
        assert!(k.not_colors.is_empty());
    }

    #[test]
    fn yellow_and_blue_clues_work_the_same_way() {
        let rules = hanabii_rules();

        let mut k = CardKnowledge::default();
        hit(&mut k, Color::Yellow);
        assert_eq!(
            k.hanabii_possible_colors(&rules),
            vec![Color::Orange, Color::Yellow, Color::Green]
        );

        let mut k = CardKnowledge::default();
        hit(&mut k, Color::Blue);
        assert_eq!(
            k.hanabii_possible_colors(&rules),
            vec![Color::Green, Color::Blue, Color::Purple]
        );

        let mut k = CardKnowledge::default();
        miss(&mut k, Color::Yellow);
        assert_eq!(
            k.hanabii_possible_colors(&rules),
            vec![Color::Red, Color::Blue, Color::Purple]
        );

        let mut k = CardKnowledge::default();
        miss(&mut k, Color::Blue);
        assert_eq!(
            k.hanabii_possible_colors(&rules),
            vec![Color::Red, Color::Orange, Color::Yellow]
        );
    }

    #[test]
    fn hits_from_two_different_primaries_pin_down_the_secondary() {
        let rules = hanabii_rules();
        for (a, b, mixed) in [
            (Color::Red, Color::Yellow, Color::Orange),
            (Color::Yellow, Color::Blue, Color::Green),
            (Color::Red, Color::Blue, Color::Purple),
        ] {
            let mut k = CardKnowledge::default();
            hit(&mut k, a);
            assert_eq!(k.hanabii_certain_color(&rules), None);
            hit(&mut k, b);
            assert_eq!(k.hanabii_certain_color(&rules), Some(mixed), "{a:?} + {b:?}");
            // Two different colors touched it, and it is emphatically not
            // the multicolor wildcard — that inference doesn't apply here.
            assert!(!k.inferred_multicolor());
            assert!(!k.could_be_multicolor(&rules));
        }
    }

    #[test]
    fn misses_from_two_primaries_pin_down_the_third_one() {
        let rules = hanabii_rules();
        for (a, b, left) in [
            (Color::Red, Color::Yellow, Color::Blue),
            (Color::Yellow, Color::Blue, Color::Red),
            (Color::Red, Color::Blue, Color::Yellow),
        ] {
            let mut k = CardKnowledge::default();
            miss(&mut k, a);
            assert_eq!(k.hanabii_certain_color(&rules), None);
            miss(&mut k, b);
            assert_eq!(k.hanabii_certain_color(&rules), Some(left), "not {a:?}, not {b:?}");
        }
    }

    #[test]
    fn a_hit_and_a_miss_leave_the_primary_and_one_secondary() {
        let rules = hanabii_rules();
        let mut k = CardKnowledge::default();
        hit(&mut k, Color::Red);
        miss(&mut k, Color::Yellow);
        // Contains red, doesn't contain yellow: red itself or purple.
        assert_eq!(k.hanabii_possible_colors(&rules), vec![Color::Red, Color::Purple]);
        assert_eq!(
            k.ruled_out_colors(&rules),
            vec![Color::Orange, Color::Yellow, Color::Green, Color::Blue]
        );

        // ...and the third primary settles it either way.
        let mut plain_red = k.clone();
        miss(&mut plain_red, Color::Blue);
        assert_eq!(plain_red.hanabii_certain_color(&rules), Some(Color::Red));
        let mut purple = k;
        hit(&mut purple, Color::Blue);
        assert_eq!(purple.hanabii_certain_color(&rules), Some(Color::Purple));
    }

    #[test]
    fn the_true_color_is_never_ruled_out_whatever_clues_arrive() {
        // Property check over every card color, and every way of having
        // been given some subset of the three primary clues (with the
        // result each one really would have had): the true color always
        // stays possible, and once all three have been given it's the only
        // possibility left.
        let rules = hanabii_rules();
        for color in rules.active_colors() {
            for subset in 0u8..8 {
                let mut k = CardKnowledge::default();
                let mut given = 0;
                for (i, primary) in Color::PRIMARIES.into_iter().enumerate() {
                    if subset & (1 << i) == 0 {
                        continue;
                    }
                    given += 1;
                    k.apply_clue_result(
                        Clue::Color(primary),
                        rules.color_clue_touches(primary, color),
                        &rules,
                    );
                }
                assert!(k.could_be_hanabii_color(color), "{color:?}, clue subset {subset:03b}");
                if given == 3 {
                    assert_eq!(k.hanabii_certain_color(&rules), Some(color));
                }
            }
        }
    }

    #[test]
    fn repeating_a_primary_clue_changes_nothing() {
        let rules = hanabii_rules();
        let mut once = CardKnowledge::default();
        hit(&mut once, Color::Red);
        let mut twice = once.clone();
        hit(&mut twice, Color::Red);
        assert_eq!(once, twice);
        assert_eq!(twice.hanabii_possible_colors(&rules).len(), 3);
    }

    #[test]
    fn number_clues_are_recorded_the_ordinary_way_in_hanabii_mode() {
        let rules = hanabii_rules();
        let mut k = CardKnowledge::default();
        k.apply_clue_result(Clue::Number(4), true, &rules);
        assert_eq!(k.known_number, Some(4));

        let mut k = CardKnowledge::default();
        k.apply_clue_result(Clue::Number(2), false, &rules);
        assert!(k.not_numbers.contains(&2));
        assert_eq!(k.hanabii_possible_colors(&rules).len(), 6, "a number says nothing about color");
    }

    #[test]
    fn ordinary_games_record_clue_results_exactly_as_before() {
        let rules = GameRules::default();

        let mut via_result = CardKnowledge::default();
        via_result.apply_clue_result(Clue::Color(Color::Red), true, &rules);
        via_result.apply_clue_result(Clue::Color(Color::Blue), false, &rules);
        via_result.apply_clue_result(Clue::Number(3), true, &rules);
        via_result.apply_clue_result(Clue::Number(1), false, &rules);

        let mut by_hand = CardKnowledge::default();
        by_hand.apply_positive(Clue::Color(Color::Red));
        by_hand.apply_negative(Clue::Color(Color::Blue));
        by_hand.apply_positive(Clue::Number(3));
        by_hand.apply_negative(Clue::Number(1));

        assert_eq!(via_result, by_hand);
        assert_eq!(via_result.known_color, Some(Color::Red));
        assert!(via_result.hit_primaries.is_empty() && via_result.missed_primaries.is_empty());
    }

    #[test]
    fn ruled_out_colors_in_an_ordinary_game_is_just_the_missed_clues() {
        let rules = GameRules { extra_colors: 2, ..Default::default() };
        let mut k = CardKnowledge::default();
        k.apply_negative(Clue::Color(Color::Purple));
        k.apply_negative(Clue::Color(Color::White));
        // Display order (white, red, orange, ...), not the order clued.
        assert_eq!(k.ruled_out_colors(&rules), vec![Color::White, Color::Purple]);

        // A miss on a color that isn't in this game never shows up.
        let plain = GameRules::default();
        assert_eq!(k.ruled_out_colors(&plain), vec![Color::White]);
    }
}
