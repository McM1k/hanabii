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
