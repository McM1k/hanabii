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
    fn an_active_extra_suit_must_also_be_ruled_out_before_inferring_black() {
        // Orange is turned on alongside black in this game, so ruling out
        // only the five base colors isn't enough — orange itself hasn't
        // been eliminated as a possibility yet.
        let rules = GameRules { black: true, orange: true, ..Default::default() };
        let mut k = CardKnowledge::default();
        for color in [Color::White, Color::Red, Color::Yellow, Color::Green, Color::Blue] {
            k.apply_negative(Clue::Color(color));
        }
        assert!(!k.inferred_black(&rules), "orange hasn't been ruled out yet");

        k.apply_negative(Clue::Color(Color::Orange));
        assert!(k.inferred_black(&rules));
    }
}
