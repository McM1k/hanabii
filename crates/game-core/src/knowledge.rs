use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::card::{Clue, Color, Number};

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
}
