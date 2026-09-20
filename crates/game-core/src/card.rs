use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Color {
    White,
    Red,
    Yellow,
    Green,
    Blue,
    /// The optional 6th suit (see [`crate::rules::GameRules::multicolor`]).
    /// Only ever appears in a deck, a hand, the discard pile, etc. when the
    /// multicolor rule is enabled for that game.
    Multicolor,
    /// The optional "black powder" suit (see
    /// [`crate::rules::GameRules::black`]). Has no color at all for clue
    /// purposes — no color clue, including naming it directly, ever
    /// touches it, the polar opposite of [`Color::Multicolor`] — and its
    /// firework is built in *descending* order, 5 down to 1, so its card
    /// distribution is mirrored too (three 5s, ..., one 1).
    Black,
    /// The optional "orange" suit (see [`crate::rules::GameRules::orange`]).
    /// A perfectly ordinary suit — ascending 1-5, normal clue matching —
    /// it's just optional.
    Orange,
    /// The optional "purple" suit (see [`crate::rules::GameRules::purple`]).
    /// Just as ordinary as [`Color::Orange`], also optional.
    Purple,
}

impl Color {
    /// The five standard suits, always in play. Does not include
    /// [`Color::Multicolor`] — see [`crate::rules::GameRules::active_colors`]
    /// for the full set of colors in play for a given game.
    pub const ALL: [Color; 5] = [
        Color::White,
        Color::Red,
        Color::Yellow,
        Color::Green,
        Color::Blue,
    ];

    /// The three primary colors of the hanabii game mode (see
    /// [`crate::rules::GameRules::hanabii`]) — the only colors a player may
    /// name in a color clue there. Everything else in that mode is mixed
    /// from these (see [`Color::primary_components`]).
    pub const PRIMARIES: [Color; 3] = [Color::Red, Color::Yellow, Color::Blue];

    /// Whether this is one of the three [`Color::PRIMARIES`].
    pub fn is_primary(self) -> bool {
        matches!(self, Color::Red | Color::Yellow | Color::Blue)
    }

    /// The primary colors this color is made of, as used by the hanabii
    /// game mode: a primary is made of just itself, and the three
    /// secondaries are mixed from two primaries each — orange is red +
    /// yellow, green is yellow + blue, purple is red + blue.
    ///
    /// A color clue in that mode touches every card whose color contains
    /// the named primary, which is exactly "the named primary is one of
    /// this card color's components". Colors that have no place on this
    /// wheel (white, multicolor, black — none of which exist in a hanabii
    /// game) are made of nothing, so no clue there ever touches them.
    pub fn primary_components(self) -> &'static [Color] {
        match self {
            Color::Red => &[Color::Red],
            Color::Yellow => &[Color::Yellow],
            Color::Blue => &[Color::Blue],
            Color::Orange => &[Color::Red, Color::Yellow],
            Color::Green => &[Color::Yellow, Color::Blue],
            Color::Purple => &[Color::Red, Color::Blue],
            Color::White | Color::Multicolor | Color::Black => &[],
        }
    }
}

/// Card ranks run 1-5. A plain alias keeps this easy to serialize and compare;
/// nothing here enforces the range at the type level, the engine does that.
pub type Number = u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Card {
    pub color: Color,
    pub number: Number,
}

/// Stable identity for one physical card for the whole game, independent of
/// which hand slot it's currently sitting in. This is how a client tracks
/// "the card I was told is red" even while its number is still unknown, and
/// how actions reference a specific card without needing hand positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardId(pub u32);

/// The two things you're allowed to clue about a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Clue {
    Color(Color),
    Number(Number),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_red_yellow_and_blue_are_primary() {
        let all = [
            Color::White,
            Color::Red,
            Color::Yellow,
            Color::Green,
            Color::Blue,
            Color::Multicolor,
            Color::Black,
            Color::Orange,
            Color::Purple,
        ];
        let primaries: Vec<Color> = all.into_iter().filter(|c| c.is_primary()).collect();
        assert_eq!(primaries, vec![Color::Red, Color::Yellow, Color::Blue]);
        for p in Color::PRIMARIES {
            assert!(p.is_primary());
        }
    }

    #[test]
    fn a_primary_is_made_of_just_itself() {
        for p in Color::PRIMARIES {
            assert_eq!(p.primary_components(), &[p]);
        }
    }

    #[test]
    fn secondaries_are_mixed_from_two_primaries_each() {
        assert_eq!(Color::Orange.primary_components(), &[Color::Red, Color::Yellow]);
        assert_eq!(Color::Green.primary_components(), &[Color::Yellow, Color::Blue]);
        assert_eq!(Color::Purple.primary_components(), &[Color::Red, Color::Blue]);
    }

    #[test]
    fn colors_off_the_wheel_are_made_of_nothing() {
        assert!(Color::White.primary_components().is_empty());
        assert!(Color::Multicolor.primary_components().is_empty());
        assert!(Color::Black.primary_components().is_empty());
    }

    #[test]
    fn every_pair_of_primaries_mixes_into_exactly_one_secondary() {
        // Red + yellow, yellow + blue and red + blue each show up in one
        // (and only one) secondary — which is what makes "touched by two
        // different primaries" identify a card's color outright.
        let secondaries = [Color::Orange, Color::Green, Color::Purple];
        for (a, b) in [
            (Color::Red, Color::Yellow),
            (Color::Yellow, Color::Blue),
            (Color::Red, Color::Blue),
        ] {
            let containing_both: Vec<_> = secondaries
                .iter()
                .filter(|s| {
                    s.primary_components().contains(&a) && s.primary_components().contains(&b)
                })
                .collect();
            assert_eq!(containing_both.len(), 1);
        }
    }
}
