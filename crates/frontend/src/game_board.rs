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
fn valid_clues(cards: &[VisibleCard]) -> (Vec<Color>, Vec<u8>) {
    let mut colors: Vec<Color> = cards.iter().filter_map(|c| c.card.map(|card| card.color)).collect();
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

#[component]
pub fn GameBoard() -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext should be provided by App");

    let (selected_target, set_selected_target) = create_signal(None::<PlayerId>);

    view! {
        <div class="game-board">
            {move || {
                let Some(view) = ctx.view.get() else {
                    return view! { <p>"Waiting for the game to start…"</p> }.into_view();
                };

                let you = view.you;
                let is_my_turn = view.current_turn == you;
                let can_act = is_my_turn && view.status == GameStatus::InProgress;
                let can_clue = can_act && view.clue_tokens > 0;
                let can_discard = can_act && view.clue_tokens < MAX_CLUE_TOKENS;
                let roster = ctx.roster.get_untracked();
                let name_of = move |id: PlayerId| {
                    roster
                        .iter()
                        .find(|(pid, _)| *pid == id)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_else(|| format!("Player {}", id.0))
                };

                let status_line = match view.status {
                    GameStatus::InProgress => {
                        if is_my_turn {
                            "Your turn.".to_string()
                        } else {
                            format!("{}'s turn.", name_of(view.current_turn))
                        }
                    }
                    GameStatus::Finished(reason) => {
                        let why = match reason {
                            EndReason::FusesExhausted => "ran out of fuses",
                            EndReason::DeckExhausted => "the deck ran out",
                            EndReason::PerfectScore => "a perfect score",
                        };
                        format!("Game over — {why}. Final score: {}/25", view.score)
                    }
                };

                let fireworks_items = Color::ALL
                    .into_iter()
                    .map(|color| {
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

                let discard_groups = Color::ALL
                    .into_iter()
                    .filter_map(|color| {
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

                let mut all_ids: Vec<PlayerId> = view.hands.keys().copied().collect();
                all_ids.sort_by_key(|id| id.0);
                let ordered_ids = turn_order(&all_ids, view.current_turn);

                // One combined pass over every seat in turn order — the top
                // of this list is always whoever plays next, your own hand
                // included, so "who plays when" reads directly top to bottom
                // rather than being split across separate "you" / "others"
                // sections.
                let hand_blocks = ordered_ids
                    .iter()
                    .map(|&pid| {
                        let is_you = pid == you;
                        let is_current = pid == view.current_turn;
                        let cards = view.hands.get(&pid).cloned().unwrap_or_default();

                        let mut classes = vec!["hand".to_string()];
                        if is_current {
                            classes.push("hand-current".to_string());
                        }

                        let now_playing = is_current.then(|| {
                            view! { <span class="now-playing">"Now playing"</span> }
                        });

                        let last_move_line = view.last_moves.get(&pid).map(|mv| {
                            view! { <span class="last-move">{describe_move(mv, &name_of)}</span> }
                        });

                        if is_you {
                            let card_items = cards
                                .iter()
                                .map(|c| {
                                    let mut parts = Vec::new();
                                    if let Some(color) = c.knowledge.known_color {
                                        parts.push(format!("{color:?}"));
                                    }
                                    if let Some(number) = c.knowledge.known_number {
                                        parts.push(number.to_string());
                                    }
                                    let hint =
                                        if parts.is_empty() { "?".to_string() } else { parts.join(" ") };
                                    let card_id = c.id;
                                    view! {
                                        <li
                                            class="card card-unknown"
                                            draggable=if can_act { "true" } else { "false" }
                                            on:dragstart=move |ev: web_sys::DragEvent| {
                                                if let Some(dt) = ev.data_transfer() {
                                                    let _ = dt.set_data("text/plain", &card_id.0.to_string());
                                                }
                                            }
                                        >
                                            <span class="card-hint">{hint}</span>
                                            <span class="card-tag">{format!("#{}", c.id.0)}</span>
                                        </li>
                                    }
                                })
                                .collect_view();

                            view! {
                                <div class=classes.join(" ")>
                                    <div class="hand-header">
                                        <h3>"Your hand"</h3>
                                        {now_playing}
                                        {last_move_line}
                                    </div>
                                    <ul class="cards">{card_items}</ul>
                                </div>
                            }
                            .into_view()
                        } else {
                            let is_selected = selected_target.get() == Some(pid);
                            if is_selected {
                                classes.push("hand-selected".to_string());
                            }

                            let card_items = cards
                                .iter()
                                .map(|c| {
                                    let card = c.card.expect("other players' cards are always visible");
                                    view! {
                                        <li class=format!("card card-{}", color_class(card.color))>
                                            {card.number.to_string()}
                                        </li>
                                    }
                                })
                                .collect_view();

                            let (valid_colors, valid_numbers) = valid_clues(&cards);
                            let clue_section = is_selected.then(|| {
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
                                                    }))
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
                                                    }))
                                                }
                                            >
                                                {number.to_string()}
                                            </button>
                                        }
                                    })
                                    .collect_view();
                                view! {
                                    <div class="clue-options">
                                        <p class="hint">"Give a clue:"</p>
                                        <div class="clue-buttons">{color_buttons}{number_buttons}</div>
                                    </div>
                                }
                            });

                            view! {
                                <div class=classes.join(" ")>
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
                                    {clue_section}
                                </div>
                            }
                            .into_view()
                        }
                    })
                    .collect_view();

                view! {
                    <div>
                        <p class="status-line">{status_line}</p>

                        <div
                            class=if can_act { "panel drop-zone".to_string() } else { "panel drop-zone disabled".to_string() }
                            on:dragover=move |ev: web_sys::DragEvent| ev.prevent_default()
                            on:drop=move |ev: web_sys::DragEvent| {
                                ev.prevent_default();
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

                        <div
                            class=if can_discard { "panel drop-zone".to_string() } else { "panel drop-zone disabled".to_string() }
                            on:dragover=move |ev: web_sys::DragEvent| ev.prevent_default()
                            on:drop=move |ev: web_sys::DragEvent| {
                                ev.prevent_default();
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

                        <div class="panel">
                            <p class="hint">"Top of the list plays next. Click another player's name to see clues you can give them."</p>
                            {hand_blocks}
                        </div>
                    </div>
                }
                    .into_view()
            }}
        </div>
    }
}
