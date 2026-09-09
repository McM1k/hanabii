use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use leptos::html::Div;
use leptos::*;

use game_core::{
    Action, CardId, ClientMessage, Clue, Color, EndReason, GameStatus, LastMove, PlayerId,
    VisibleCard, MAX_CLUE_TOKENS, MAX_FUSE_TOKENS,
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

/// The distinct colors and numbers actually present in a hand — the only
/// clues that wouldn't be rejected by the engine as touching zero cards.
///
/// A multicolor card counts as *every* color when receiving a clue (see
/// `GameState::apply_clue` in game-core), so once a hand holds one, every
/// base color becomes a legal clue for that hand even if none of its other
/// cards are actually that color — but multicolor itself can never be the
/// color named in a clue, so it's never included here.
fn valid_clues(cards: &[VisibleCard]) -> (Vec<Color>, Vec<u8>) {
    let has_multicolor = cards
        .iter()
        .any(|c| c.card.map(|card| card.color) == Some(Color::Multicolor));

    let mut colors: Vec<Color> = if has_multicolor {
        Color::ALL.to_vec()
    } else {
        cards.iter().filter_map(|c| c.card.map(|card| card.color)).collect()
    };
    colors.sort();
    colors.dedup();

    let mut numbers: Vec<u8> = cards.iter().filter_map(|c| c.card.map(|card| card.number)).collect();
    numbers.sort();
    numbers.dedup();
    (colors, numbers)
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

/// A small burst icon that fills in more as `progress` (0-5) increases —
/// an original take on the physical Hanabi cards, where laying out a suit's
/// cards in order reveals progressively more of a firework illustration.
fn firework_burst(progress: u8) -> impl IntoView {
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

    let coarse = move || {
        let Some(view) = ctx.view.get() else {
            return Vec::<View>::new().into_view();
        };

        let is_my_turn = view.current_turn == you;
        let can_act = is_my_turn && view.status == GameStatus::InProgress;
        let can_discard = can_act && view.clue_tokens < MAX_CLUE_TOKENS;
        let active_colors = view.rules.active_colors();

        let status_line = match view.status {
            GameStatus::InProgress => None,
            GameStatus::Finished(reason) => {
                let why = match reason {
                    EndReason::FusesExhausted => "ran out of fuses",
                    EndReason::DeckExhausted => "the deck ran out",
                    EndReason::PerfectScore => "a perfect score",
                };
                let max_score = active_colors.len() as u8 * 5;
                Some(format!(
                    "Game over — {why}. Final score: {}/{max_score}",
                    view.score
                ))
            }
        };

        let fireworks_items = active_colors
            .iter()
            .map(|&color| {
                let n = *view.fireworks.get(&color).unwrap_or(&0);
                let label = if n == 0 { "—".to_string() } else { n.to_string() };
                view! {
                    <div class=format!("firework firework-{}", color_class(color))>
                        <span class="firework-label">{format!("{color:?}")}</span>
                        {firework_burst(n)}
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
                        view! { <div class="discard-groups">{discard_groups}</div> }.into_view()
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

                            if is_you {
                                cards
                                    .iter()
                                    .map(|c| {
                                        let mut parts = Vec::new();
                                        if c.knowledge.inferred_multicolor() {
                                            // Matched two *different* color
                                            // clues — no real single-colored
                                            // card could do that, so this is
                                            // a hard deduction, not a guess.
                                            // Abbreviated: the card is too
                                            // narrow to fit "Multicolor".
                                            parts.push("Multi".to_string());
                                        } else if let Some(color) = c.knowledge.known_color {
                                            parts.push(format!("{color:?}"));
                                        }
                                        if let Some(number) = c.knowledge.known_number {
                                            parts.push(number.to_string());
                                        }
                                        let hint = if parts.is_empty() {
                                            "?".to_string()
                                        } else {
                                            parts.join(" ")
                                        };
                                        let card_id = c.id;
                                        view! {
                                            <li
                                                class="card card-unknown"
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
                                                <span class="card-tag">{format!("#{}", c.id.0)}</span>
                                            </li>
                                        }
                                        .into_view()
                                    })
                                    .collect_view()
                            } else {
                                cards
                                    .iter()
                                    .map(|c| {
                                        let card = c.card.expect("other players' cards are always visible");
                                        view! {
                                            <li class=format!("card card-{}", color_class(card.color))>
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

                            let (valid_colors, valid_numbers) = valid_clues(&cards);
                            let color_buttons = valid_colors
                                .iter()
                                .map(|&color| {
                                    view! {
                                        <button
                                            class=format!("clue-btn card-{}", color_class(color))
                                            disabled=!can_clue
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

    view! {
        <div class="game-board">
            <Show
                when=move || ctx.view.with(Option::is_some)
                fallback=|| view! { <p>"Waiting for the game to start…"</p> }
            >
                {move || ready_board(ctx, selected_target, set_selected_target, is_dragging, set_is_dragging)}
            </Show>
        </div>
    }
}
