use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use leptos::html::Div;
use leptos::*;

use game_core::{
    next_expected_rank, points_for, Action, Card, CardId, ClientMessage, Clue, Color, EndReason,
    GameRules, GameStatus, LastMove, PlayerId, VisibleCard, MAX_CLUE_TOKENS, MAX_FUSE_TOKENS,
};

use crate::ws::AppContext;

fn color_class(c: Color) -> &'static str {
    match c {
        Color::White => "white",
        Color::Red => "red",
        Color::Yellow => "yellow",
        Color::Green => "green",
        Color::Blue => "blue",
        Color::Multicolor => "multicolor",
        Color::Black => "black",
        Color::Orange => "orange",
        Color::Purple => "purple",
    }
}

/// A single-letter abbreviation for the "ruled out" marks on own-hand
/// cards. Only ever called with a base or plain-optional color in
/// practice — a color clue can never name Multicolor or Black directly, so
/// neither can ever end up in a card's `not_colors` set — but the match
/// stays exhaustive.
fn color_initial(c: Color) -> &'static str {
    match c {
        Color::White => "W",
        Color::Red => "R",
        Color::Yellow => "Y",
        Color::Green => "G",
        Color::Blue => "B",
        Color::Multicolor => "M",
        Color::Black => "K",
        Color::Orange => "O",
        Color::Purple => "P",
    }
}

fn pips(current: u8, max: u8) -> String {
    let filled = "\u{25cf}".repeat(current as usize);
    let empty = "\u{25cb}".repeat((max - current) as usize);
    format!("{filled}{empty}")
}

/// Pulls the dragged card's id back out of a drop event. `dragstart` stores
/// it as plain text via `DataTransfer::set_data`; this is the other half.
fn dragged_card_id(ev: &web_sys::DragEvent) -> Option<CardId> {
    let data_transfer = ev.data_transfer()?;
    let id_str = data_transfer.get_data("text/plain").ok()?;
    id_str.parse::<u32>().ok().map(CardId)
}

/// The distinct colors and numbers that would actually touch something in
/// a hand — the only clues that wouldn't be rejected by the engine as
/// touching zero cards.
///
/// The colors offered come from `GameRules::cluable_colors` (every active
/// color but Multicolor and Black ordinarily; just red, yellow and blue in
/// hanabii mode), each kept only if `GameRules::color_clue_touches` says it
/// would touch at least one card here. That one definition covers all the
/// special cases: a multicolor card counts as *every* color when receiving
/// a clue, so a hand holding one makes every cluable color valid; black is
/// touched by nothing, so it never makes a color valid; and in hanabii mode
/// a red clue is valid for a hand holding only an orange card, since red is
/// one of orange's ingredients.
fn valid_clues(cards: &[VisibleCard], rules: &GameRules) -> (Vec<Color>, Vec<u8>) {
    let visible: Vec<Card> = cards.iter().filter_map(|c| c.card).collect();

    let mut colors: Vec<Color> = rules
        .cluable_colors()
        .into_iter()
        .filter(|&clue| {
            visible
                .iter()
                .any(|card| rules.color_clue_touches(clue, card.color))
        })
        .collect();
    colors.sort();
    colors.dedup();

    let mut numbers: Vec<u8> = visible.iter().map(|card| card.number).collect();
    numbers.sort();
    numbers.dedup();
    (colors, numbers)
}

/// Joins names as "a", "a and b" or "a, b and c".
fn join_with_and(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// What a color clue would touch, worded for a button tooltip — only worth
/// showing where it isn't obvious from the button itself, i.e. in hanabii
/// mode, where a red clue also touches orange and purple cards. Empty
/// (which browsers show no tooltip for) in an ordinary game.
fn clue_touch_tooltip(rules: &GameRules, clue: Color) -> String {
    if !rules.hanabii {
        return String::new();
    }
    let names: Vec<String> = rules
        .active_colors()
        .into_iter()
        .filter(|&color| rules.color_clue_touches(clue, color))
        .map(|color| format!("{color:?}").to_lowercase())
        .collect();
    format!("Touches {} cards", join_with_and(&names))
}

/// What hanabii mode draws around one of your own cards while its color is
/// still uncertain: a ring made of every color the card could still be —
/// that's the whole of what the clues have told you, so no separate marks
/// are needed. A red clue that touches a card leaves red, orange and purple
/// (three equal arcs); a yellow miss on top of that leaves red and purple
/// (two). The ring spins on a card a color clue has *touched*, so clued
/// cards catch the eye; a card that has only been *missed* by clues (which
/// still narrows it down — a red miss leaves yellow, green and blue) gets
/// the same ring standing still.
///
/// There's no ring on a card no color clue has narrowed down yet, and none
/// once the clues leave a single color — the whole card face fills in with
/// that color instead (see the `card-<color>` classes). `None` outside
/// hanabii mode.
#[derive(Debug, PartialEq)]
struct HanabiiRing {
    /// Every color the card could still be, in the game's display order.
    colors: Vec<Color>,
    /// Whether a color clue has actually touched the card.
    spinning: bool,
}

fn hanabii_ring(knowledge: &game_core::CardKnowledge, rules: &GameRules) -> Option<HanabiiRing> {
    if !rules.hanabii {
        return None;
    }
    let possible = knowledge.hanabii_possible_colors(rules);
    if possible.len() <= 1 || possible.len() == rules.active_colors().len() {
        return None;
    }
    Some(HanabiiRing {
        colors: possible,
        // At most one primary can have touched a card that's still
        // uncertain (two different hits always settle it), so this is just
        // "has anything touched it".
        spinning: !knowledge.hit_primaries.is_empty(),
    })
}

impl HanabiiRing {
    /// The classes that draw the ring (see `.card-ring` in style.css).
    fn class(&self) -> &'static str {
        if self.spinning {
            "card-ring card-ring-spin"
        } else {
            "card-ring"
        }
    }

    /// The `--ring-stops` custom property the ring's gradient is built
    /// from: one hard-edged arc per color, all the same size.
    fn style(&self) -> String {
        let n = self.colors.len() as f64;
        let stops: Vec<String> = self
            .colors
            .iter()
            .enumerate()
            .map(|(i, &color)| {
                format!(
                    "var(--{}-fw) {:.2}% {:.2}%",
                    color_class(color),
                    100.0 * i as f64 / n,
                    100.0 * (i + 1) as f64 / n,
                )
            })
            .collect();
        format!("--ring-stops: {}", stops.join(", "))
    }
}

/// All seated players, starting from whoever's turn it is right now and
/// wrapping around in normal turn order. This is what lets the hand list
/// show "who plays when" just by reading top to bottom.
fn turn_order(all_ids: &[PlayerId], current_turn: PlayerId) -> Vec<PlayerId> {
    let start = all_ids.iter().position(|&id| id == current_turn).unwrap_or(0);
    let mut ordered = Vec::with_capacity(all_ids.len());
    ordered.extend_from_slice(&all_ids[start..]);
    ordered.extend_from_slice(&all_ids[..start]);
    ordered
}

fn describe_move(mv: &LastMove, name_of: &dyn Fn(PlayerId) -> String) -> String {
    match mv {
        LastMove::Clue {
            target,
            clue,
            touched_count,
        } => {
            let about = match clue {
                Clue::Color(c) => format!("{c:?}"),
                Clue::Number(n) => n.to_string(),
            };
            let cards_word = if *touched_count == 1 { "card" } else { "cards" };
            format!(
                "Clued {} about {about} ({touched_count} {cards_word})",
                name_of(*target)
            )
        }
        LastMove::Play { card, success } => {
            let verb = if *success { "Played" } else { "Misplayed" };
            format!("{verb} {:?} {}", card.color, card.number)
        }
        LastMove::Discard { card } => {
            format!("Discarded {:?} {}", card.color, card.number)
        }
    }
}

/// Endpoints for an 8-ray burst radiating from (20, 20) in a 40x40 viewBox,
/// precomputed (not done at runtime) at three different radii so each
/// firework's "reveal" can grow in both ray count and length as it fills in.
/// Order is E, SE, S, SW, W, NW, N, NE.
const RAYS_SHORT: [(&str, &str); 8] = [
    ("26", "20"),
    ("24.2", "24.2"),
    ("20", "26"),
    ("15.8", "24.2"),
    ("14", "20"),
    ("15.8", "15.8"),
    ("20", "14"),
    ("24.2", "15.8"),
];
const RAYS_MEDIUM: [(&str, &str); 8] = [
    ("32", "20"),
    ("28.5", "28.5"),
    ("20", "32"),
    ("11.5", "28.5"),
    ("8", "20"),
    ("11.5", "11.5"),
    ("20", "8"),
    ("28.5", "11.5"),
];
const RAYS_FULL: [(&str, &str); 8] = [
    ("35", "20"),
    ("30.6", "30.6"),
    ("20", "35"),
    ("9.4", "30.6"),
    ("5", "20"),
    ("9.4", "9.4"),
    ("20", "5"),
    ("30.6", "9.4"),
];

/// A denser, larger burst reserved for a suit that's actually *complete*
/// (every card of that colour has been played) rather than merely at the
/// same "full" stage `RAYS_FULL` renders at progress 5. The two used to be
/// indistinguishable, which looked fine for an ordinary 5-card suit (where
/// progress 5 always meant done) but reads as wrong for a `six_cards` suit
/// sitting at progress 5 with one play still to go. Twelve rays instead of
/// eight, reaching further out toward the icon's edge.
const RAYS_BRILLIANT: [(&str, &str); 12] = [
    ("37", "20"),
    ("34.7", "28.5"),
    ("28.5", "34.7"),
    ("20", "37"),
    ("11.5", "34.7"),
    ("5.3", "28.5"),
    ("3", "20"),
    ("5.3", "11.5"),
    ("11.5", "5.3"),
    ("20", "3"),
    ("28.5", "5.3"),
    ("34.7", "11.5"),
];

/// A small burst icon that fills in more as `progress` (0-5) increases —
/// an original take on the physical Hanabi cards, where laying out a suit's
/// cards in order reveals progressively more of a firework illustration.
/// `complete` overrides all of that once the suit is actually finished
/// (see `RAYS_BRILLIANT`), rendering a bigger, denser burst that also picks
/// up its own color via the `firework-burst--complete` class instead of
/// just inheriting the tile's ordinary dark/light icon color — the same
/// "something notable happened" accent already used for the drawn-card
/// highlight elsewhere in this app.
fn firework_burst(progress: u8, complete: bool) -> impl IntoView {
    if complete {
        let ray_lines = RAYS_BRILLIANT
            .iter()
            .map(|&(x2, y2)| {
                view! {
                    <line
                        x1="20"
                        y1="20"
                        x2=x2
                        y2=y2
                        stroke="currentColor"
                        stroke-width="2.2"
                        stroke-linecap="round"
                    />
                }
            })
            .collect_view();
        let tip_dots = RAYS_BRILLIANT
            .iter()
            .map(|&(x, y)| view! { <circle cx=x cy=y r="1.6" fill="currentColor" /> })
            .collect_view();

        return view! {
            <svg class="firework-burst firework-burst--complete" viewBox="0 0 40 40">
                {ray_lines}
                <circle cx="20" cy="20" r="6" fill="currentColor" />
                {tip_dots}
            </svg>
        }
        .into_view();
    }

    let (rays, dot_r): (&[(&str, &str)], &str) = match progress {
        0 => (&[], "2"),
        1 => (&RAYS_SHORT[0..1], "3"),
        2 => (&RAYS_SHORT[0..2], "3.5"),
        3 => (&RAYS_MEDIUM[0..4], "4"),
        4 => (&RAYS_MEDIUM[0..6], "4.5"),
        _ => (&RAYS_FULL[0..8], "5"),
    };

    let ray_lines = rays
        .iter()
        .map(|&(x2, y2)| {
            view! {
                <line
                    x1="20"
                    y1="20"
                    x2=x2
                    y2=y2
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                />
            }
        })
        .collect_view();

    let tip_dots = (progress >= 5).then(|| {
        RAYS_FULL
            .iter()
            .step_by(2)
            .map(|&(x, y)| view! { <circle cx=x cy=y r="1.5" fill="currentColor" /> })
            .collect_view()
    });

    view! {
        <svg class="firework-burst" viewBox="0 0 40 40">
            {ray_lines}
            <circle cx="20" cy="20" r=dot_r fill="currentColor" />
            {tip_dots}
        </svg>
    }
    .into_view()
}

/// How long the hand-swap slide takes. Kept in one place since it has to
/// match between the CSS `transition` we set from Rust and (loosely) how
/// long it feels right for a handful of DOM nodes gliding past each other.
const HAND_SLIDE_MS: u32 = 350;

/// Builds the whole in-progress board — fireworks, discard pile, drop
/// zones, and the turn-ordered hand list — once a `PlayerView` exists.
/// Constructed exactly once per game (see the `<Show>` in `GameBoard`
/// below), which matters a lot for the hand list: it's rendered through
/// `<For>` so each player keeps the *same* DOM node turn after turn, which
/// is what lets the FLIP effect animate a hand sliding to its new spot
/// instead of the whole list just popping into a new order.
fn ready_board(
    ctx: AppContext,
    selected_target: ReadSignal<Option<PlayerId>>,
    set_selected_target: WriteSignal<Option<PlayerId>>,
    is_dragging: ReadSignal<bool>,
    set_is_dragging: WriteSignal<bool>,
    hovered_clue: ReadSignal<Option<Clue>>,
    set_hovered_clue: WriteSignal<Option<Clue>>,
) -> impl IntoView {
    let initial = ctx
        .view
        .get_untracked()
        .expect("ready_board is only built once ctx.view is populated");
    let you = initial.you;

    let mut all_ids: Vec<PlayerId> = initial.hands.keys().copied().collect();
    all_ids.sort_by_key(|id| id.0);

    let name_of = move |id: PlayerId| -> String {
        ctx.roster
            .get_untracked()
            .iter()
            .find(|(pid, _)| *pid == id)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| format!("Player {}", id.0))
    };

    // A stable node ref per seat, created once — `<For>` re-uses (moves,
    // never recreates) the underlying `<div class="hand">` for a given key
    // as the turn order rotates, so these keep pointing at the same real
    // DOM element for the whole game.
    let hand_refs: HashMap<PlayerId, NodeRef<Div>> =
        all_ids.iter().map(|&pid| (pid, create_node_ref::<Div>())).collect();

    // FLIP bookkeeping: the vertical position each hand was measured at the
    // last time this ran, so the next run can tell how far each one moved.
    let last_tops: Rc<RefCell<HashMap<PlayerId, f64>>> = Rc::new(RefCell::new(HashMap::new()));

    {
        let hand_refs = hand_refs.clone();
        let last_tops = last_tops.clone();
        create_effect(move |_| {
            // Re-run this effect exactly when the hand list can have
            // reordered — i.e. every state update, since every accepted
            // action advances whose turn it is.
            let Some(_view) = ctx.view.get() else {
                return;
            };

            let mut new_tops = HashMap::with_capacity(hand_refs.len());
            for (&pid, node_ref) in &hand_refs {
                let Some(el) = node_ref.get() else { continue };
                let top = el.get_bounding_client_rect().top();
                new_tops.insert(pid, top);

                let old_top = last_tops.borrow().get(&pid).copied();
                if let Some(old_top) = old_top {
                    let delta = old_top - top;
                    if delta.abs() > 1.0 {
                        // `HtmlElement::style` is leptos_dom's own builder
                        // method (it consumes and returns `Self`), not the
                        // web-sys `CssStyleDeclaration` getter — so each
                        // call is chained/reassigned rather than going
                        // through a separate `.style()` handle.
                        //
                        // Jump back to where it visually was, with
                        // transitions off so this doesn't itself animate...
                        let el = el
                            .style("transition", "none")
                            .style("transform", format!("translateY({delta:.1}px)"));
                        // ...force the browser to actually commit that
                        // frame before we change anything else...
                        let _ = el.get_bounding_client_rect();
                        // ...then animate back to its real (natural, zero
                        // offset) position.
                        let _ = el
                            .style("transition", format!("transform {HAND_SLIDE_MS}ms ease"))
                            .style("transform", "translateY(0)");
                    }
                }
            }
            *last_tops.borrow_mut() = new_tops;
        });
    }

    // Whoever was selected as a clue target only makes sense for the turn
    // during which they were picked — once the turn moves on (for any
    // reason: a clue given, a play, a discard), clear it so the next turn
    // starts without a stale target and its clue panel still showing.
    create_effect(move |prev_turn: Option<PlayerId>| match ctx.view.get() {
        Some(view) => {
            if prev_turn.is_some_and(|prev| prev != view.current_turn) {
                set_selected_target.set(None);
            }
            view.current_turn
        }
        None => prev_turn.unwrap_or(you),
    });

    // Remembers each firework's previous value and the discard pile's
    // previous size, purely to detect "did this just change" so the
    // relevant tile can briefly flash — the current value alone (inside
    // `coarse`, which rebuilds fresh on every state update) can't tell
    // "just happened" from "already true a while ago" without this.
    let previous_fireworks: Rc<RefCell<HashMap<Color, u8>>> = Rc::new(RefCell::new(HashMap::new()));
    let previous_discard_count: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));

    let coarse = move || {
        let Some(view) = ctx.view.get() else {
            return Vec::<View>::new().into_view();
        };

        let is_my_turn = view.current_turn == you;
        let can_act = is_my_turn && view.status == GameStatus::InProgress;
        let can_discard = can_act && view.clue_tokens < MAX_CLUE_TOKENS;
        let active_colors = view.rules.active_colors();

        // Which fireworks just gained a card, and whether the discard
        // pile just grew — compared against what was stored last render,
        // so this only fires on the render where it actually happened.
        let just_played: HashMap<Color, bool> = {
            let mut prev = previous_fireworks.borrow_mut();
            let changed: HashMap<Color, bool> = active_colors
                .iter()
                .map(|&color| {
                    let current = *view.fireworks.get(&color).unwrap_or(&0);
                    let last = *prev.get(&color).unwrap_or(&0);
                    (color, current > last)
                })
                .collect();
            for &color in &active_colors {
                prev.insert(color, *view.fireworks.get(&color).unwrap_or(&0));
            }
            changed
        };
        let just_discarded = {
            let mut prev = previous_discard_count.borrow_mut();
            let changed = view.discard_pile.len() > *prev;
            *prev = view.discard_pile.len();
            changed
        };

        let status_line = match view.status {
            GameStatus::InProgress => None,
            GameStatus::Finished(reason) => {
                let why = match reason {
                    EndReason::FusesExhausted => "ran out of fuses",
                    EndReason::DeckExhausted => "the deck ran out",
                    EndReason::PerfectScore => "a perfect score",
                };
                let max_score = view.rules.max_score();
                Some(format!(
                    "Game over — {why}. Final score: {}/{max_score}",
                    view.score
                ))
            }
        };

        let fireworks_items = active_colors
            .iter()
            .map(|&color| {
                let top = *view.fireworks.get(&color).unwrap_or(&0);
                let label = if top == 0 { "—".to_string() } else { top.to_string() };
                // The burst illustration fills in based on how many cards
                // of this suit have actually been played — the same
                // "points this suit is worth" computation the score itself
                // uses, since a normal suit's progress is just its top
                // rank but a reverse suit (Black) counts down instead.
                let progress = points_for(color, top, view.rules.max_rank());
                // Distinct from "progress is at its highest displayed
                // stage" — with `six_cards` on, a suit can sit at progress
                // 5 for one more play before it's actually done.
                let complete = next_expected_rank(color, top, view.rules.max_rank()).is_none();
                let suit_label = if color == Color::Black {
                    format!("{color:?} \u{2193}")
                } else {
                    format!("{color:?}")
                };
                let short_badge = view.rules.is_short(color).then(|| {
                    view! {
                        <span class="firework-short-badge" title="Short deck: only 1 of each card">
                            "1×"
                        </span>
                    }
                });
                let mut tile_class = format!("firework firework-{}", color_class(color));
                if *just_played.get(&color).unwrap_or(&false) {
                    tile_class.push_str(" firework-flash");
                }
                if complete {
                    tile_class.push_str(" firework-complete");
                }
                view! {
                    <div class=tile_class>
                        {short_badge}
                        <span class="firework-label">{suit_label}</span>
                        {firework_burst(progress, complete)}
                        <span class="firework-value">{label}</span>
                    </div>
                }
            })
            .collect_view();

        let discard_groups = active_colors
            .iter()
            .filter_map(|&color| {
                let mut numbers: Vec<u8> = view
                    .discard_pile
                    .iter()
                    .filter(|c| c.color == color)
                    .map(|c| c.number)
                    .collect();
                if numbers.is_empty() {
                    return None;
                }
                numbers.sort_unstable();
                let chips = numbers
                    .iter()
                    .map(|n| {
                        view! {
                            <span class=format!("chip card-{}", color_class(color))>
                                {n.to_string()}
                            </span>
                        }
                    })
                    .collect_view();
                Some(view! {
                    <div class="discard-row">
                        <span class="discard-color-label">{format!("{color:?}")}</span>
                        <span class="discard-chips">{chips}</span>
                    </div>
                })
            })
            .collect_view();

        let mut nodes: Vec<View> = Vec::new();
        if let Some(line) = status_line {
            nodes.push(view! { <p class="status-line">{line}</p> }.into_view());
        }

        nodes.push(
            view! {
                <div
                    class=move || {
                        let mut classes = vec!["panel", "drop-zone"];
                        if !can_act {
                            classes.push("disabled");
                        } else if is_dragging.get() {
                            classes.push("drag-active");
                        }
                        classes.join(" ")
                    }
                    on:dragover=move |ev: web_sys::DragEvent| ev.prevent_default()
                    on:drop=move |ev: web_sys::DragEvent| {
                        ev.prevent_default();
                        set_is_dragging.set(false);
                        if let Some(card_id) = dragged_card_id(&ev) {
                            ctx.send(ClientMessage::Action(Action::Play { card_id }));
                        }
                    }
                >
                    <div class="fireworks">{fireworks_items}</div>
                    <p class="tokens">
                        "Clues " <span class="pip-row">{pips(view.clue_tokens, MAX_CLUE_TOKENS)}</span>
                        "   Fuses " <span class="pip-row">{pips(view.fuse_tokens, MAX_FUSE_TOKENS)}</span>
                        "   Deck: " {view.draw_pile_count}
                    </p>
                    <p class="hint">"Drag a card here to play it."</p>
                </div>
            }
            .into_view(),
        );

        nodes.push(
            view! {
                <div
                    class=move || {
                        let mut classes = vec!["panel", "drop-zone"];
                        if !can_discard {
                            classes.push("disabled");
                        } else if is_dragging.get() {
                            classes.push("drag-active");
                        }
                        classes.join(" ")
                    }
                    on:dragover=move |ev: web_sys::DragEvent| ev.prevent_default()
                    on:drop=move |ev: web_sys::DragEvent| {
                        ev.prevent_default();
                        set_is_dragging.set(false);
                        if let Some(card_id) = dragged_card_id(&ev) {
                            ctx.send(ClientMessage::Action(Action::Discard { card_id }));
                        }
                    }
                >
                    <h3>"Discard pile"</h3>
                    {if view.discard_pile.is_empty() {
                        view! { <p class="hint">"Nothing discarded yet."</p> }.into_view()
                    } else {
                        let class = if just_discarded {
                            "discard-groups discard-flash"
                        } else {
                            "discard-groups"
                        };
                        view! { <div class=class>{discard_groups}</div> }.into_view()
                    }}
                    <p class="hint">"Drag a card here to discard it."</p>
                </div>
            }
            .into_view(),
        );

        nodes.into_view()
    };

    let hand_refs_for_children = hand_refs.clone();

    view! {
        <div>
            {coarse}

            <div class="panel">
                <p class="hint">"Top of the list plays next. Click another player's name to see clues you can give them."</p>
                <For
                    each=move || {
                        let Some(view) = ctx.view.get() else { return Vec::new() };
                        let mut ids: Vec<PlayerId> = view.hands.keys().copied().collect();
                        ids.sort_by_key(|id| id.0);
                        turn_order(&ids, view.current_turn)
                    }
                    key=|pid: &PlayerId| *pid
                    children=move |pid: PlayerId| {
                        let node_ref = hand_refs_for_children[&pid];
                        let is_you = pid == you;
                        // Remembers this hand's card ids from the last
                        // render, purely to spot newly-drawn ones — `None`
                        // means "nothing rendered yet", so the very first
                        // render (the initial deal) doesn't get flagged as
                        // a draw.
                        let previously_seen_cards: Rc<RefCell<Option<HashSet<CardId>>>> =
                            Rc::new(RefCell::new(None));
                        // Which of this hand's cards were drawn recently
                        // enough to still be worth calling out. A genuine
                        // signal (not just a CSS animation fired at render
                        // time) so the highlight is reliably visible for a
                        // fixed stretch — added the instant a draw is
                        // detected, removed by its own timeout — regardless
                        // of whether the underlying `<li>` for that card is
                        // a freshly-created DOM node or one Leptos reused
                        // from a previous render.
                        let (recently_drawn, set_recently_drawn) = create_signal(HashSet::<CardId>::new());

                        // Dedicated to detecting draws and scheduling their
                        // highlight — kept separate from `card_items` below
                        // (which only *reads* `recently_drawn`) so nothing
                        // both reads and writes the same signal from within
                        // one reactive scope.
                        create_effect(move |_| {
                            let Some(view) = ctx.view.get() else { return };
                            let current_ids: HashSet<CardId> = view
                                .hands
                                .get(&pid)
                                .map(|cards| cards.iter().map(|c| c.id).collect())
                                .unwrap_or_default();

                            let newly_drawn: HashSet<CardId> = {
                                let mut prev = previously_seen_cards.borrow_mut();
                                let result = match prev.as_ref() {
                                    Some(old_ids) => current_ids.difference(old_ids).copied().collect(),
                                    None => HashSet::new(),
                                };
                                *prev = Some(current_ids);
                                result
                            };

                            if !newly_drawn.is_empty() {
                                set_recently_drawn.update(|set| set.extend(newly_drawn.iter().copied()));
                                for id in newly_drawn {
                                    set_timeout(
                                        move || {
                                            set_recently_drawn.update(|set| {
                                                set.remove(&id);
                                            });
                                        },
                                        std::time::Duration::from_millis(1500),
                                    );
                                }
                            }
                        });

                        let hand_class = move || {
                            let is_current = ctx.view.get().map(|v| v.current_turn == pid).unwrap_or(false);
                            let mut classes = vec!["hand".to_string()];
                            if is_current {
                                classes.push("hand-current".to_string());
                            }
                            if !is_you && selected_target.get() == Some(pid) {
                                classes.push("hand-selected".to_string());
                            }
                            classes.join(" ")
                        };

                        let now_playing = move || {
                            let is_current = ctx.view.get().map(|v| v.current_turn == pid).unwrap_or(false);
                            is_current.then(|| view! { <span class="now-playing">"Now playing"</span> })
                        };

                        let last_move_line = move || {
                            ctx.view.get().and_then(|v| {
                                v.last_moves.get(&pid).map(|mv| {
                                    view! { <span class="last-move">{describe_move(mv, &name_of)}</span> }
                                })
                            })
                        };

                        let card_items = move || {
                            let Some(view) = ctx.view.get() else {
                                return Vec::<View>::new().into_view();
                            };
                            let cards = view.hands.get(&pid).cloned().unwrap_or_default();
                            let can_act =
                                view.current_turn == you && view.status == GameStatus::InProgress;
                            let recently_drawn_now = recently_drawn.get();

                            if is_you {
                                cards
                                    .iter()
                                    .map(|c| {
                                        let mut parts = Vec::new();
                                        // The card's own background already
                                        // shows its color (or gradient, for
                                        // multicolor, or the dark black
                                        // styling) once it's known or
                                        // inferred — same as how other
                                        // players' cards never repeat their
                                        // color as text either. Only the
                                        // ambiguity flag and the number
                                        // aren't otherwise visible, so those
                                        // are all that go in the hint text.
                                        if c.knowledge.could_be_multicolor(&view.rules) {
                                            // A single color clue could
                                            // still be explained by the
                                            // multicolor wildcard rather
                                            // than the color itself — flag
                                            // that ambiguity rather than
                                            // silently picking one. Stops
                                            // applying the moment any other
                                            // color clue comes back
                                            // negative, since a multicolor
                                            // card could never miss one.
                                            parts.push("M?".to_string());
                                        }
                                        if let Some(number) = c.knowledge.known_number {
                                            parts.push(number.to_string());
                                        }
                                        let hint = if parts.is_empty() {
                                            "?".to_string()
                                        } else {
                                            parts.join(" ")
                                        };

                                        // In hanabii mode a color clue never simply
                                        // "makes the card red": a red hit means red,
                                        // orange *or* purple. So the card's color only
                                        // counts as known once the primary-color clues
                                        // so far leave a single possibility (red and
                                        // yellow both hit → orange; red and yellow both
                                        // missed → blue). Until then the card keeps its
                                        // neutral face and shows what it could still
                                        // be as a ring around it (see `hanabii_ring`).
                                        let hanabii_color = if view.rules.hanabii {
                                            c.knowledge.hanabii_certain_color(&view.rules)
                                        } else {
                                            None
                                        };
                                        let color_settled = if view.rules.hanabii {
                                            hanabii_color.is_some()
                                        } else {
                                            c.knowledge.known_color.is_some()
                                                || c.knowledge.inferred_black(&view.rules)
                                        };
                                        // Ruled-out colors/numbers, shown
                                        // only while that aspect is still
                                        // uncertain — once the color (or a
                                        // black/multicolor inference) or
                                        // number is already known above,
                                        // repeating what it *isn't* is just
                                        // clutter. Hanabii mode has no
                                        // struck-through color marks at all:
                                        // its ring already lists everything
                                        // the card could still be.
                                        let struck_colors: Vec<Color> = if color_settled || view.rules.hanabii {
                                            Vec::new()
                                        } else {
                                            c.knowledge.ruled_out_colors(&view.rules)
                                        };
                                        let not_colors_row = (!struck_colors.is_empty()).then(|| {
                                            let marks = struck_colors
                                                .iter()
                                                .map(|&nc| {
                                                    view! {
                                                        <span class=format!(
                                                            "not-mark not-mark-{}",
                                                            color_class(nc),
                                                        )>
                                                            {color_initial(nc)}
                                                        </span>
                                                    }
                                                })
                                                .collect_view();
                                            view! { <span class="not-row">{marks}</span> }
                                        });
                                        let not_numbers_row = c
                                            .knowledge
                                            .known_number
                                            .is_none()
                                            .then(|| {
                                                let mut ruled_out: Vec<u8> =
                                                    c.knowledge.not_numbers.iter().copied().collect();
                                                ruled_out.sort_unstable();
                                                (!ruled_out.is_empty()).then(|| {
                                                    let marks = ruled_out
                                                        .iter()
                                                        .map(|&n| {
                                                            view! {
                                                                <span class="not-mark">{n.to_string()}</span>
                                                            }
                                                        })
                                                        .collect_view();
                                                    view! { <span class="not-row">{marks}</span> }
                                                })
                                            })
                                            .flatten();

                                        // Color the card face itself once
                                        // enough is known, same as other
                                        // players see it — "card-own" carries
                                        // the stacked-info layout regardless
                                        // of which of these applies.
                                        let color_class_name = if view.rules.hanabii {
                                            match hanabii_color {
                                                Some(color) => format!("card-{}", color_class(color)),
                                                None => "card-unknown".to_string(),
                                            }
                                        } else if c.knowledge.inferred_multicolor() {
                                            "card-multicolor".to_string()
                                        } else if c.knowledge.inferred_black(&view.rules) {
                                            "card-black".to_string()
                                        } else if let Some(color) = c.knowledge.known_color {
                                            format!("card-{}", color_class(color))
                                        } else {
                                            "card-unknown".to_string()
                                        };

                                        let card_id = c.id;
                                        let mut li_class = format!("card card-own {color_class_name}");
                                        // Hanabii mode's ring of still-possible colors,
                                        // if the card has one (see `hanabii_ring`).
                                        let ring = hanabii_ring(&c.knowledge, &view.rules);
                                        if let Some(ring) = &ring {
                                            li_class.push(' ');
                                            li_class.push_str(ring.class());
                                        }
                                        let ring_style = ring.as_ref().map(HanabiiRing::style).unwrap_or_default();
                                        if recently_drawn_now.contains(&card_id) {
                                            li_class.push_str(" card-recent-draw");
                                        }
                                        view! {
                                            <li
                                                class=li_class
                                                style=ring_style
                                                draggable=if can_act { "true" } else { "false" }
                                                on:dragstart=move |ev: web_sys::DragEvent| {
                                                    if let Some(dt) = ev.data_transfer() {
                                                        let _ = dt.set_data("text/plain", &card_id.0.to_string());
                                                    }
                                                    set_is_dragging.set(true);
                                                }
                                                on:dragend=move |_ev: web_sys::DragEvent| {
                                                    set_is_dragging.set(false);
                                                }
                                            >
                                                <span class="card-hint">{hint}</span>
                                                {not_colors_row}
                                                {not_numbers_row}
                                                <span class="card-tag">{format!("#{}", c.id.0)}</span>
                                            </li>
                                        }
                                        .into_view()
                                    })
                                    .collect_view()
                            } else {
                                // Only preview a hover on the hand the clue
                                // buttons actually belong to — other hands
                                // may coincidentally share a color/number
                                // but aren't what's about to be clued.
                                let preview_clue = if selected_target.get() == Some(pid) {
                                    hovered_clue.get()
                                } else {
                                    None
                                };
                                cards
                                    .iter()
                                    .map(|c| {
                                        let card = c.card.expect("other players' cards are always visible");
                                        // Same definition of "touches" the engine uses,
                                        // so the preview can't drift from what the
                                        // clue would actually do — including, in
                                        // hanabii mode, a red clue lighting up
                                        // orange and purple cards too.
                                        let is_targeted = preview_clue
                                            .map(|clue| view.rules.clue_touches(clue, card))
                                            .unwrap_or(false);
                                        let mut class = if is_targeted {
                                            format!("card card-{} card-clue-target", color_class(card.color))
                                        } else {
                                            format!("card card-{}", color_class(card.color))
                                        };
                                        if recently_drawn_now.contains(&c.id) {
                                            class.push_str(" card-recent-draw");
                                        }
                                        view! {
                                            <li class=class>
                                                {card.number.to_string()}
                                            </li>
                                        }
                                        .into_view()
                                    })
                                    .collect_view()
                            }
                        };

                        let clue_section = move || {
                            if is_you || selected_target.get() != Some(pid) {
                                return None;
                            }
                            let view = ctx.view.get()?;
                            let cards = view.hands.get(&pid).cloned().unwrap_or_default();
                            let can_act =
                                view.current_turn == you && view.status == GameStatus::InProgress;
                            let can_clue = can_act && view.clue_tokens > 0;

                            let (valid_colors, valid_numbers) = valid_clues(&cards, &view.rules);
                            let color_buttons = valid_colors
                                .iter()
                                .map(|&color| {
                                    let tooltip = clue_touch_tooltip(&view.rules, color);
                                    view! {
                                        <button
                                            class=format!("clue-btn card-{}", color_class(color))
                                            title=tooltip
                                            disabled=!can_clue
                                            on:mouseenter=move |_| {
                                                set_hovered_clue.set(Some(Clue::Color(color)));
                                            }
                                            on:mouseleave=move |_| set_hovered_clue.set(None)
                                            on:click=move |_| {
                                                ctx.send(ClientMessage::Action(Action::Clue {
                                                    target: pid,
                                                    clue: Clue::Color(color),
                                                }));
                                            }
                                        >
                                            {format!("{color:?}")}
                                        </button>
                                    }
                                })
                                .collect_view();
                            let number_buttons = valid_numbers
                                .iter()
                                .map(|&number| {
                                    view! {
                                        <button
                                            class="clue-btn"
                                            disabled=!can_clue
                                            on:mouseenter=move |_| {
                                                set_hovered_clue.set(Some(Clue::Number(number)));
                                            }
                                            on:mouseleave=move |_| set_hovered_clue.set(None)
                                            on:click=move |_| {
                                                ctx.send(ClientMessage::Action(Action::Clue {
                                                    target: pid,
                                                    clue: Clue::Number(number),
                                                }));
                                            }
                                        >
                                            {number.to_string()}
                                        </button>
                                    }
                                })
                                .collect_view();

                            Some(view! {
                                <div class="clue-options">
                                    <p class="hint">"Give a clue:"</p>
                                    <div class="clue-buttons">{color_buttons}</div>
                                    <div class="clue-buttons">{number_buttons}</div>
                                </div>
                            })
                        };

                        if is_you {
                            view! {
                                <div class=hand_class _ref=node_ref>
                                    <div class="hand-main">
                                        <div class="hand-header">
                                            <h3>"Your hand"</h3>
                                            {now_playing}
                                            {last_move_line}
                                        </div>
                                        <ul class="cards">{card_items}</ul>
                                    </div>
                                </div>
                            }
                            .into_view()
                        } else {
                            view! {
                                <div class=hand_class _ref=node_ref>
                                    <div class="hand-main">
                                        <div class="hand-header">
                                            <h3
                                                class="player-name"
                                                on:click=move |_| {
                                                    set_selected_target.update(|t| {
                                                        *t = if *t == Some(pid) { None } else { Some(pid) };
                                                    });
                                                }
                                            >
                                                {name_of(pid)}
                                            </h3>
                                            {now_playing}
                                            {last_move_line}
                                        </div>
                                        <ul class="cards">{card_items}</ul>
                                    </div>
                                    {clue_section}
                                </div>
                            }
                            .into_view()
                        }
                    }
                />
            </div>
        </div>
    }
}

#[component]
pub fn GameBoard() -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext should be provided by App");

    let (selected_target, set_selected_target) = create_signal(None::<PlayerId>);
    // Tracks whether a card is currently being dragged, so the play/discard
    // drop zones can highlight themselves while a drag is in progress.
    let (is_dragging, set_is_dragging) = create_signal(false);
    // Which clue button (if any) is currently being hovered, so the cards
    // it would touch can be outlined as a preview before it's actually given.
    let (hovered_clue, set_hovered_clue) = create_signal(None::<Clue>);

    view! {
        <div class="game-board">
            <Show
                when=move || ctx.view.with(Option::is_some)
                fallback=|| view! { <p>"Waiting for the game to start…"</p> }
            >
                {move || {
                    ready_board(
                        ctx,
                        selected_target,
                        set_selected_target,
                        is_dragging,
                        set_is_dragging,
                        hovered_clue,
                        set_hovered_clue,
                    )
                }}
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::CardKnowledge;

    fn hand(cards: &[(Color, u8)]) -> Vec<VisibleCard> {
        cards
            .iter()
            .enumerate()
            .map(|(i, &(color, number))| VisibleCard {
                id: CardId(i as u32),
                card: Some(Card { color, number }),
                knowledge: CardKnowledge::default(),
            })
            .collect()
    }

    fn hanabii() -> GameRules {
        GameRules { hanabii: true, ..Default::default() }.normalized()
    }

    #[test]
    fn hanabii_offers_only_the_primaries_that_would_touch_something() {
        // Two orange cards: red and yellow both touch them (orange is red +
        // yellow), blue touches neither.
        let (colors, numbers) = valid_clues(&hand(&[(Color::Orange, 1), (Color::Orange, 2)]), &hanabii());
        assert_eq!(colors, vec![Color::Red, Color::Yellow]);
        assert_eq!(numbers, vec![1, 2]);
    }

    #[test]
    fn hanabii_never_offers_a_secondary_color_as_a_clue() {
        // Every secondary in the hand — and each primary is a valid clue
        // for two of them — but orange, green and purple themselves are
        // never on offer.
        let cards = hand(&[(Color::Green, 1), (Color::Purple, 2), (Color::Orange, 3)]);
        let (colors, _) = valid_clues(&cards, &hanabii());
        assert_eq!(colors, vec![Color::Red, Color::Yellow, Color::Blue]);
    }

    #[test]
    fn hanabii_single_cards_offer_exactly_their_ingredients() {
        assert_eq!(valid_clues(&hand(&[(Color::Red, 3)]), &hanabii()).0, vec![Color::Red]);
        assert_eq!(
            valid_clues(&hand(&[(Color::Green, 3)]), &hanabii()).0,
            vec![Color::Yellow, Color::Blue]
        );
        assert_eq!(
            valid_clues(&hand(&[(Color::Purple, 3)]), &hanabii()).0,
            vec![Color::Red, Color::Blue]
        );
    }

    #[test]
    fn ordinary_games_offer_the_colors_present_in_the_hand() {
        let cards = hand(&[(Color::Red, 1), (Color::Blue, 2), (Color::Blue, 3)]);
        let (colors, numbers) = valid_clues(&cards, &GameRules::default());
        assert_eq!(colors, vec![Color::Red, Color::Blue]);
        assert_eq!(numbers, vec![1, 2, 3]);
    }

    #[test]
    fn a_multicolor_card_makes_every_cluable_color_valid() {
        let rules = GameRules { multicolor: true, extra_colors: 1, ..Default::default() };
        let (colors, _) = valid_clues(&hand(&[(Color::Multicolor, 3)]), &rules);
        // Everything in play except Multicolor itself (never named in a
        // clue) — and no White, which `extra_colors: 1` drops.
        assert_eq!(
            colors,
            vec![Color::Red, Color::Yellow, Color::Green, Color::Blue, Color::Orange, Color::Purple]
        );
    }

    #[test]
    fn a_black_card_never_makes_a_color_clue_valid() {
        let rules = GameRules { black: true, ..Default::default() };
        let (colors, numbers) = valid_clues(&hand(&[(Color::Black, 5)]), &rules);
        assert!(colors.is_empty());
        assert_eq!(numbers, vec![5]);
    }

    fn knowledge_after(results: &[(Color, bool)]) -> game_core::CardKnowledge {
        let rules = hanabii();
        let mut k = CardKnowledge::default();
        for &(primary, touched) in results {
            k.apply_clue_result(Clue::Color(primary), touched, &rules);
        }
        k
    }

    fn ring(colors: &[Color], spinning: bool) -> Option<HanabiiRing> {
        Some(HanabiiRing { colors: colors.to_vec(), spinning })
    }

    #[test]
    fn a_touched_card_gets_a_spinning_ring_of_every_color_it_could_be() {
        let rules = hanabii();
        // Red touched it: red, orange or purple.
        let k = knowledge_after(&[(Color::Red, true)]);
        assert_eq!(
            hanabii_ring(&k, &rules),
            ring(&[Color::Red, Color::Orange, Color::Purple], true)
        );
        // ...and a yellow miss on top of that leaves red or purple.
        let k = knowledge_after(&[(Color::Red, true), (Color::Yellow, false)]);
        assert_eq!(hanabii_ring(&k, &rules), ring(&[Color::Red, Color::Purple], true));
        // The other two primaries work the same way.
        let k = knowledge_after(&[(Color::Yellow, true)]);
        assert_eq!(
            hanabii_ring(&k, &rules),
            ring(&[Color::Orange, Color::Yellow, Color::Green], true)
        );
        let k = knowledge_after(&[(Color::Blue, true)]);
        assert_eq!(
            hanabii_ring(&k, &rules),
            ring(&[Color::Green, Color::Blue, Color::Purple], true)
        );
    }

    #[test]
    fn a_card_that_was_only_missed_gets_the_same_ring_standing_still() {
        let rules = hanabii();
        // A red miss rules out red, orange and purple: yellow, green or blue.
        let k = knowledge_after(&[(Color::Red, false)]);
        assert_eq!(
            hanabii_ring(&k, &rules),
            ring(&[Color::Yellow, Color::Green, Color::Blue], false)
        );
        let k = knowledge_after(&[(Color::Blue, false)]);
        assert_eq!(
            hanabii_ring(&k, &rules),
            ring(&[Color::Red, Color::Orange, Color::Yellow], false)
        );
    }

    #[test]
    fn a_card_no_color_clue_has_narrowed_down_has_no_ring() {
        let rules = hanabii();
        assert_eq!(hanabii_ring(&CardKnowledge::default(), &rules), None);
        // A number clue says nothing about color.
        let mut k = CardKnowledge::default();
        k.apply_clue_result(Clue::Number(3), true, &rules);
        assert_eq!(hanabii_ring(&k, &rules), None);
    }

    #[test]
    fn a_card_whose_color_is_certain_has_no_ring_because_the_whole_card_fills_in() {
        let rules = hanabii();
        // Red + yellow hit: orange.
        let k = knowledge_after(&[(Color::Red, true), (Color::Yellow, true)]);
        assert_eq!(hanabii_ring(&k, &rules), None);
        // Red hit, yellow and blue missed: plain red.
        let k = knowledge_after(&[(Color::Red, true), (Color::Yellow, false), (Color::Blue, false)]);
        assert_eq!(hanabii_ring(&k, &rules), None);
        // Red and yellow both missed: blue, without ever being touched.
        let k = knowledge_after(&[(Color::Red, false), (Color::Yellow, false)]);
        assert_eq!(hanabii_ring(&k, &rules), None);
    }

    #[test]
    fn there_is_never_a_ring_outside_hanabii_mode() {
        let k = knowledge_after(&[(Color::Red, true)]);
        assert_eq!(hanabii_ring(&k, &GameRules::default()), None);
    }

    #[test]
    fn every_reachable_ring_has_two_or_three_colors_and_spins_exactly_when_touched() {
        // Over every card color and every subset of the three primary
        // clues (with the results they'd really have): whenever a ring is
        // drawn it lists the card's true color, has two or three arcs, and
        // spins exactly when some clue touched the card.
        let rules = hanabii();
        for color in rules.active_colors() {
            for subset in 0u8..8 {
                let mut results = Vec::new();
                for (i, primary) in Color::PRIMARIES.into_iter().enumerate() {
                    if subset & (1 << i) != 0 {
                        results.push((primary, rules.color_clue_touches(primary, color)));
                    }
                }
                let k = knowledge_after(&results);
                if let Some(ring) = hanabii_ring(&k, &rules) {
                    assert!(ring.colors.contains(&color), "{color:?} {results:?}");
                    assert!((2..=3).contains(&ring.colors.len()), "{color:?} {results:?}");
                    assert_eq!(ring.spinning, results.iter().any(|&(_, touched)| touched));
                }
            }
        }
    }

    #[test]
    fn the_ring_is_drawn_as_equal_hard_edged_arcs_one_per_color() {
        let three = HanabiiRing {
            colors: vec![Color::Red, Color::Orange, Color::Purple],
            spinning: true,
        };
        assert_eq!(
            three.style(),
            "--ring-stops: var(--red-fw) 0.00% 33.33%, var(--orange-fw) 33.33% 66.67%, var(--purple-fw) 66.67% 100.00%"
        );
        let two = HanabiiRing { colors: vec![Color::Red, Color::Purple], spinning: false };
        assert_eq!(
            two.style(),
            "--ring-stops: var(--red-fw) 0.00% 50.00%, var(--purple-fw) 50.00% 100.00%"
        );
    }

    #[test]
    fn only_touched_cards_spin() {
        assert_eq!(HanabiiRing { colors: vec![], spinning: true }.class(), "card-ring card-ring-spin");
        assert_eq!(HanabiiRing { colors: vec![], spinning: false }.class(), "card-ring");
    }

    #[test]
    fn clue_tooltips_name_everything_a_hanabii_clue_touches() {
        let rules = hanabii();
        assert_eq!(clue_touch_tooltip(&rules, Color::Red), "Touches red, orange and purple cards");
        assert_eq!(clue_touch_tooltip(&rules, Color::Yellow), "Touches orange, yellow and green cards");
        assert_eq!(clue_touch_tooltip(&rules, Color::Blue), "Touches green, blue and purple cards");
    }

    #[test]
    fn clue_tooltips_are_left_off_in_ordinary_games() {
        assert_eq!(clue_touch_tooltip(&GameRules::default(), Color::Red), "");
    }

    #[test]
    fn join_with_and_reads_naturally() {
        let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(join_with_and(&names(&[])), "");
        assert_eq!(join_with_and(&names(&["red"])), "red");
        assert_eq!(join_with_and(&names(&["red", "blue"])), "red and blue");
        assert_eq!(join_with_and(&names(&["red", "orange", "purple"])), "red, orange and purple");
    }
}
