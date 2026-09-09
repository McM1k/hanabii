use leptos::*;

use game_core::ClientMessage;

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

#[component]
pub fn Lobby() -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext should be provided by App");

    let on_start = move |_| ctx.send(ClientMessage::StartGame);

    // Rules are server-authoritative and shared by the whole lobby: rather
    // than trust an uncontrolled checkbox, we always flip the last
    // confirmed `ctx.rules` and let the server's `RulesUpdated` echo be
    // what actually moves the checkbox — same pattern as every other
    // action in this app going through a round trip rather than updating
    // local state optimistically.
    let on_toggle_multicolor = move |_| {
        let mut rules = ctx.rules.get_untracked();
        rules.multicolor = !rules.multicolor;
        ctx.send(ClientMessage::SetRules { rules });
    };

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
                <label class="rule-toggle">
                    <input
                        type="checkbox"
                        prop:checked=move || ctx.rules.get().multicolor
                        on:change=on_toggle_multicolor
                    />
                    <span>"Multicolor suit"</span>
                </label>
                <p class="hint">"Adds a 6th suit that's wild for color clues but can't be clued directly. Max score becomes 30."</p>
            </div>

            <button on:click=on_start disabled=move || ctx.roster.get().len() < 2>
                "Start game"
            </button>
            <p class="hint">"Needs at least 2 players. Anyone can start once you're ready."</p>
        </div>
    }
}
