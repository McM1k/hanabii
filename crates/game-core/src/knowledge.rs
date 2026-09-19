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

    /// True once two or more *different* color clues have touched this
    /// card. A real single-colored card can only ever match clues of its
    /// own color, so matching two different ones is only possible for the
    /// multicolor suit — this is a hard deduction from the clue history,
    /// not a guess, and holds however the card knowledge was assembled.
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
}
