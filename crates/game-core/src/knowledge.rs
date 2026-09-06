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
}

impl CardKnowledge {
    pub fn apply_positive(&mut self, clue: Clue) {
        match clue {
            Clue::Color(c) => self.known_color = Some(c),
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
}
