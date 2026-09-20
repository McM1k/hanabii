use leptos::*;

use game_core::{ClientMessage, GameRules};

use crate::ws::{connect, AppContext};

#[component]
pub fn JoinScreen() -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext should be provided by App");

    let (name, set_name) = create_signal(String::new());
    let (room_code, set_room_code) = create_signal(String::new());

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let name = name.get_untracked().trim().to_string();
        let code = room_code.get_untracked().trim().to_string();
        if name.is_empty() || code.is_empty() {
            ctx.status.set("Enter a name and a room code.".to_string());
            return;
        }
        connect(ctx, code, name);
    };

    view! {
        <form class="panel join-form" on:submit=on_submit>
            <label>
                "Your name"
                <input
                    type="text"
                    prop:value=move || name.get()
                    on:input=move |ev| set_name.set(event_target_value(&ev))
                />
            </label>
            <label>
                "Room code"
                <input
                    type="text"
                    prop:value=move || room_code.get()
                    on:input=move |ev| set_room_code.set(event_target_value(&ev))
                />
            </label>
            <button type="submit">"Join"</button>
            <p class="hint">"Share the same room code with everyone you're playing with."</p>
        </form>
    }
}

/// The class for one of the ordinary option blocks below: dimmed and
/// inert-looking while the hanabii mode is on, since that mode is a fixed
/// preset that replaces all of them (the server swaps the preset in when
/// it's picked and echoes it back, so the locked controls show exactly what
/// the mode plays with).
fn rule_group_class(ctx: AppContext) -> &'static str {
    if ctx.rules.get().hanabii {
        "rule-group rule-group-locked"
    } else {
        "rule-group"
    }
}

/// Renders the "hanabii mode" lobby control: one on/off toggle for the
/// fixed preset described in `GameRules::hanabii` — six colors, six-card
/// suits, and primary-color-only clues where orange, green and purple are
/// mixed from red, yellow and blue. While it's on, every other option is
/// locked (see `rule_group_class`).
///
/// Same server-authoritative pattern as every other toggle here: this only
/// ever asks for the change, and the `RulesUpdated` echo is what moves the
/// checkbox. Picking the mode just flags it — the server replaces the rest
/// with the preset. Un-picking goes back to a plain default game rather
/// than leaving the preset's options ticked behind it, so unticking really
/// does mean "back to normal".
fn hanabii_mode_control(ctx: AppContext) -> impl IntoView {
    let on_toggle = move |_| {
        let rules = ctx.rules.get_untracked();
        let rules = if rules.hanabii {
            GameRules::default()
        } else {
            GameRules { hanabii: true, ..rules }
        };
        ctx.send(ClientMessage::SetRules { rules });
    };

    view! {
        <div class="rule-group">
            <label class="rule-toggle">
                <input
                    type="checkbox"
                    prop:checked=move || ctx.rules.get().hanabii
                    on:change=on_toggle
                />
                <span>"Hanabii mode"</span>
            </label>
            <p class="hint">
                "The real deal: six colors (red, orange, yellow, green, blue, purple) with six cards each — three 1s, two each of 2 to 5, one 6 — for a max score of 36. Only the primary colors (red, yellow, blue) can be clued, and every other color is mixed from them: orange is red + yellow, green is yellow + blue, purple is red + blue. So a red clue touches every red, orange and purple card. Picking it locks all the other options below."
            </p>
        </div>
    }
}

/// Renders one optional suit's lobby controls: a main on/off toggle, its
/// blurb, and a "short deck" sub-toggle (one copy of each rank instead of
/// the usual distribution) that's only meaningful — and only enabled —
/// once the suit itself is on. Both are locked while the hanabii mode is
/// on (see `hanabii_mode_control`).
///
/// `get`/`set` read and write the suit's own on/off flag; `get_short`/
/// `set_short` do the same for its short-deck flag. Rules are
/// server-authoritative and shared by the whole lobby: rather than trust
/// an uncontrolled checkbox, every toggle here always flips the last
/// confirmed `ctx.rules` and lets the server's `RulesUpdated` echo be what
/// actually moves the checkbox — same pattern as every other action in
/// this app going through a round trip rather than updating local state
/// optimistically.
fn suit_rule_toggle(
    ctx: AppContext,
    label: &'static str,
    blurb: &'static str,
    get: impl Fn(&GameRules) -> bool + Copy + 'static,
    set: impl Fn(&mut GameRules, bool) + Copy + 'static,
    get_short: impl Fn(&GameRules) -> bool + Copy + 'static,
    set_short: impl Fn(&mut GameRules, bool) + Copy + 'static,
) -> impl IntoView {
    let on_toggle = move |_| {
        let mut rules = ctx.rules.get_untracked();
        let was_on = get(&rules);
        set(&mut rules, !was_on);
        ctx.send(ClientMessage::SetRules { rules });
    };
    let on_toggle_short = move |_| {
        let mut rules = ctx.rules.get_untracked();
        let was_on = get_short(&rules);
        set_short(&mut rules, !was_on);
        ctx.send(ClientMessage::SetRules { rules });
    };

    view! {
        <div class=move || rule_group_class(ctx)>
            <label class="rule-toggle">
                <input
                    type="checkbox"
                    prop:checked=move || get(&ctx.rules.get())
                    disabled=move || ctx.rules.get().hanabii
                    on:change=on_toggle
                />
                <span>{label}</span>
            </label>
            <p class="hint">{blurb}</p>
            <label class="rule-toggle rule-toggle-sub">
                <input
                    type="checkbox"
                    prop:checked=move || get_short(&ctx.rules.get())
                    disabled=move || ctx.rules.get().hanabii || !get(&ctx.rules.get())
                    on:change=on_toggle_short
                />
                <span>"Only 1 of each card (harder)"</span>
            </label>
        </div>
    }
}

/// Renders the "extra colors" lobby control: a 0-2 count selector (picked
/// in priority from ordinary, non-special suits — orange first, then
/// purple) and a shared "short deck" sub-toggle that applies uniformly to
/// however many are added. Kept separate from `suit_rule_toggle` since its
/// shape is a count, not a plain on/off flag.
fn extra_colors_control(ctx: AppContext) -> impl IntoView {
    let on_change_count = move |ev: leptos::ev::Event| {
        let value: u8 = event_target_value(&ev).parse().unwrap_or(0).min(2);
        let mut rules = ctx.rules.get_untracked();
        rules.extra_colors = value;
        ctx.send(ClientMessage::SetRules { rules });
    };
    let on_toggle_short = move |_| {
        let mut rules = ctx.rules.get_untracked();
        rules.extra_colors_short = !rules.extra_colors_short;
        ctx.send(ClientMessage::SetRules { rules });
    };

    view! {
        <div class=move || rule_group_class(ctx)>
            <label class="rule-select">
                <span>"Extra colors"</span>
                <select
                    prop:value=move || ctx.rules.get().extra_colors.to_string()
                    disabled=move || ctx.rules.get().hanabii
                    on:change=on_change_count
                >
                    <option value="0">"0"</option>
                    <option value="1">"1"</option>
                    <option value="2">"2"</option>
                </select>
            </label>
            <p class="hint">"0: just the plain five (max score 25). 1: Orange and Purple both come in and White drops out to make room — 6 suits, max score 30. 2: White comes back too, all seven suits at once, max score 35. (6 per suit instead of 5 with six-card suits.)"</p>
            <label class="rule-toggle rule-toggle-sub">
                <input
                    type="checkbox"
                    prop:checked=move || ctx.rules.get().extra_colors_short
                    disabled=move || {
                        let rules = ctx.rules.get();
                        rules.hanabii || rules.extra_colors == 0
                    }
                    on:change=on_toggle_short
                />
                <span>"Only 1 of each card (harder)"</span>
            </label>
        </div>
    }
}

/// Renders the "six-card suits" lobby control: a single on/off toggle that
/// extends *every* active suit's distribution by one rank (see
/// `GameRules::six_cards`), base five included. Unlike the suits above,
/// this isn't its own suit to turn on — it's a modifier that applies
/// uniformly to whichever suits end up active, so it gets a plain on/off
/// block of its own rather than reusing `suit_rule_toggle`'s shape.
fn six_cards_control(ctx: AppContext) -> impl IntoView {
    let on_toggle = move |_| {
        let mut rules = ctx.rules.get_untracked();
        rules.six_cards = !rules.six_cards;
        ctx.send(ClientMessage::SetRules { rules });
    };

    view! {
        <div class=move || rule_group_class(ctx)>
            <label class="rule-toggle">
                <input
                    type="checkbox"
                    prop:checked=move || ctx.rules.get().six_cards
                    disabled=move || ctx.rules.get().hanabii
                    on:change=on_toggle
                />
                <span>"Six-card suits"</span>
            </label>
            <p class="hint">
                "Adds a 6th card to every active suit. A suit's old unique 5 becomes a pair, and 6 becomes the new unique top card — mirrored for Black powder, where 6 becomes the abundant starting card instead. Adds 1 to the max score per active suit."
            </p>
        </div>
    }
}

#[component]
pub fn Lobby() -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext should be provided by App");

    let on_start = move |_| ctx.send(ClientMessage::StartGame);

    view! {
        <div class="panel">
            <h2>"Waiting for players"</h2>
            <ul class="roster">
                {move || {
                    let you = ctx.my_id.get();
                    ctx.roster
                        .get()
                        .into_iter()
                        .map(|(id, name)| {
                            let suffix = if Some(id) == you { " (you)" } else { "" };
                            view! { <li>{name}{suffix}</li> }
                        })
                        .collect_view()
                }}
            </ul>

            <div class="rules-picker">
                <h3>"Game mode"</h3>
                {hanabii_mode_control(ctx)}

                <h3>"House rules"</h3>

                {suit_rule_toggle(
                    ctx,
                    "Multicolor suit",
                    "Adds a 6th suit that's wild for color clues but can't be clued directly. Adds 5 to the max score (6 with six-card suits).",
                    |r| r.multicolor,
                    |r, v| r.multicolor = v,
                    |r| r.multicolor_short,
                    |r, v| r.multicolor_short = v,
                )}
                {suit_rule_toggle(
                    ctx,
                    "Black powder suit",
                    "Adds a suit with no color at all — color clues never touch it — played 5 down to 1 instead of 1 up to 5 (6 down to 1 with six-card suits). Adds 5 to the max score (6 with six-card suits).",
                    |r| r.black,
                    |r, v| r.black = v,
                    |r| r.black_short,
                    |r, v| r.black_short = v,
                )}
                {extra_colors_control(ctx)}
                {six_cards_control(ctx)}
            </div>

            <button on:click=on_start disabled=move || ctx.roster.get().len() < 2>
                "Start game"
            </button>
            <p class="hint">"Needs at least 2 players. Anyone can start once you're ready."</p>
        </div>
    }
}
