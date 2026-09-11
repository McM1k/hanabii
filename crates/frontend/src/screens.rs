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

/// Renders one optional suit's lobby controls: a main on/off toggle, its
/// blurb, and a "short deck" sub-toggle (one copy of each rank instead of
/// the usual distribution) that's only meaningful — and only enabled —
/// once the suit itself is on.
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
        <div class="rule-group">
            <label class="rule-toggle">
                <input type="checkbox" prop:checked=move || get(&ctx.rules.get()) on:change=on_toggle />
                <span>{label}</span>
            </label>
            <p class="hint">{blurb}</p>
            <label class="rule-toggle rule-toggle-sub">
                <input
                    type="checkbox"
                    prop:checked=move || get_short(&ctx.rules.get())
                    disabled=move || !get(&ctx.rules.get())
                    on:change=on_toggle_short
                />
                <span>"Only 1 of each card (harder)"</span>
            </label>
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
                <h3>"House rules"</h3>

                {suit_rule_toggle(
                    ctx,
                    "Multicolor suit",
                    "Adds a 6th suit that's wild for color clues but can't be clued directly. Adds 5 to the max score.",
                    |r| r.multicolor,
                    |r, v| r.multicolor = v,
                    |r| r.multicolor_short,
                    |r, v| r.multicolor_short = v,
                )}
                {suit_rule_toggle(
                    ctx,
                    "Black powder suit",
                    "Adds a suit with no color at all — color clues never touch it — played 5 down to 1 instead of 1 up to 5. Adds 5 to the max score.",
                    |r| r.black,
                    |r, v| r.black = v,
                    |r| r.black_short,
                    |r, v| r.black_short = v,
                )}
                {suit_rule_toggle(
                    ctx,
                    "Orange suit",
                    "Adds an ordinary extra suit — no special behavior, just more to keep track of. Adds 5 to the max score.",
                    |r| r.orange,
                    |r, v| r.orange = v,
                    |r| r.orange_short,
                    |r, v| r.orange_short = v,
                )}
                {suit_rule_toggle(
                    ctx,
                    "Purple suit",
                    "Adds another ordinary extra suit, same as orange. Adds 5 to the max score.",
                    |r| r.purple,
                    |r, v| r.purple = v,
                    |r| r.purple_short,
                    |r, v| r.purple_short = v,
                )}
            </div>

            <button on:click=on_start disabled=move || ctx.roster.get().len() < 2>
                "Start game"
            </button>
            <p class="hint">"Needs at least 2 players. Anyone can start once you're ready."</p>
        </div>
    }
}
