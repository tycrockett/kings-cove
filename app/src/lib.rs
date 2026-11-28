//! King's Cove client: create or join a match by room code, wait in the
//! lobby, then play on the board. Every rule is checked by the server.

mod art;
mod board;
mod fx;
mod hud;
mod nudge;
mod palette;
mod rules;
mod sound;
mod state;
mod synth;

use kings_cove_engine::{
    PlayerId, balance,
    balance::{Amount, MapShape, MapSize},
    bot::Style,
};
use kings_cove_protocol::MatchView;
use nsh_core::prelude::*;

use crate::{
    hud::{bold, label, pill},
    palette::{hex, hexa},
    state::AppState,
};

#[native_module("kings-cove/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

#[component]
pub fn App() -> impl IntoElement {
    let app = AppState::new();
    // Only a change of screen rebuilds it; new game states update in place.
    let screen = computed(move || {
        let in_session = app.session.with(|s| s.is_some());
        let started = app.view.with(|v| v.as_ref().map(|v| v.game.is_some()));
        (in_session, started)
    });
    view()
        .flex_1()
        .bg(hex(palette::TABLE))
        .child(dynamic(move || {
            Some(match screen.get() {
                (false, _) => view().flex_1().child(water_backdrop()).child(home(app)).into_element(),
                (true, None) => connecting(app).into_element(),
                (true, Some(false)) => lobby(app).into_element(),
                (true, Some(true)) => hud::game_screen(app).into_element(),
            })
        }))
        .child(hud::rulebook(app))
}

/// The home screen's sea of water tiles, behind everything; drawn again
/// only when its size changes.
fn water_backdrop() -> impl IntoElement {
    let size = signal((0.0_f32, 0.0_f32));
    canvas(move || {
        let (w, h) = size.get();
        art::water_tiles(Painter::new(), w, h)
    })
    .absolute()
    .inset_0()
    .pointer_events(PointerEvents::None)
    .on_layout(move |f: &Bounds<Points>| size.set((f.size.width.0, f.size.height.0)))
}

fn field(placeholder: &str, value: Signal<String>) -> impl IntoElement {
    nsh_core::element::input()
        .value(value)
        .placeholder(placeholder.to_owned())
        .on_change(move |t: &str| value.set(t.to_owned()))
        .h(px(46.0))
        .px(px(14.0))
        .rounded(px(12.0))
        .bg(hex(0xfffdf7))
        .border_1()
        .border_color(hexa(palette::INK, 0.2))
        .text_size(px(16.0))
        .text_color(hex(palette::INK))
}

fn card() -> View {
    view().bg(hex(palette::PAPER)).rounded(px(18.0)).p(px(18.0)).gap(px(12.0)).shadow_lg()
}

/// A titled row of chips, one per option, `current` picked; pressing another
/// calls `pick` with it, unless the row is not `enabled`.
fn choice<T: Copy + PartialEq + 'static>(
    title: &str,
    options: &[T],
    current: T,
    enabled: bool,
    text: fn(T) -> String,
    pick: impl Fn(T) + Copy + 'static,
) -> View {
    let chips: Vec<View> = options
        .iter()
        .map(|&option| {
            let on = option == current;
            let chip = view()
                .flex_1()
                .items_center()
                .py(px(8.0))
                .rounded(px(12.0))
                .bg(hex(if on { palette::INK } else { palette::PAPER_DIM }))
                .opacity(if on || enabled { 1.0 } else { 0.5 })
                .child(bold(text(option), 14.0, if on { 0xffffff } else { palette::INK }));
            if on || !enabled { chip } else { chip.active(|s| s.opacity(0.7)).on_press(move || pick(option)) }
        })
        .collect();
    view().gap(px(6.0)).child(label(title, 12.0, palette::MUTED)).child(view().flex_row().gap(px(6.0)).child(chips))
}

fn home(app: AppState) -> impl IntoElement {
    let name = app.name;
    let code = app.join_code;
    let show_server = signal(false);
    let can_create = move || !name.get().trim().is_empty();
    let can_join = move || can_create() && code.get().trim().len() >= 4;
    scroll().flex_1().child(
        view().p_safe_area().px(px(20.0)).py(px(28.0)).items_center().child(
            view()
                .w_full()
                .max_w(px(hud::MENU_WIDTH))
                .gap(px(18.0))
                .child(
                    view()
                        .items_center()
                        .gap(px(4.0))
                        .pt(px(24.0))
                        .child(bold("King's Cove", 34.0, palette::PAPER))
                        .child(label("Build, trade and defend an island", 14.0, 0xc9d3cf))
                        .child(label(format!("v{} · {}", env!("CARGO_PKG_VERSION"), env!("KC_BUILD")), 11.0, 0x8fa39c)),
                )
                .child(
                    card()
                        .child(label("Your name", 12.0, palette::MUTED))
                        .child(field("Name", name))
                        .child(dynamic(move || {
                            let ok = can_create() && !app.busy.get();
                            Some(pill("Create match", palette::GO, 0xffffff, ok, move || {
                                app.create(name.get_untracked())
                            }))
                        }))
                        // Development: a match alone against a rival who only passes.
                        .child(cfg!(debug_assertions).then(|| {
                            dynamic(move || {
                                let ok = can_create() && !app.busy.get();
                                Some(pill("Practice alone (dev)", palette::INK, 0xffffff, ok, move || {
                                    app.practice(name.get_untracked())
                                }))
                            })
                        }))
                        .child(view().h(px(1.0)).bg(hexa(palette::INK, 0.12)))
                        .child(label("Join with a room code", 12.0, palette::MUTED))
                        .child(view().flex_row().gap(px(8.0)).child(view().flex_1().child(field("CODE", code))).child(
                            dynamic(move || {
                                let ok = can_join() && !app.busy.get();
                                Some(pill("Join", 0x2f6db5, 0xffffff, ok, move || {
                                    app.join(code.get_untracked(), name.get_untracked())
                                }))
                            }),
                        )),
                )
                .child(
                    view()
                        .gap(px(8.0))
                        .child(
                            label("Settings", 12.0, 0xc9d3cf)
                                .self_center()
                                .on_press(move || show_server.update(|s| *s = !*s)),
                        )
                        .child(dynamic(move || {
                            show_server.get().then(|| {
                                card()
                                    .child(label("Server", 12.0, palette::MUTED))
                                    .child(field("http://host:8787", app.server))
                                    .child(hud::sound_settings(app))
                            })
                        })),
                )
                .child(rules_card(app))
                .child(dynamic(move || app.toast.get().map(|t| bold(t, 13.0, palette::GOLD).self_center()))),
        ),
    )
}

fn rules_card(app: AppState) -> impl IntoElement {
    card()
        .gap(px(8.0))
        .child(bold("How to play", 14.0, palette::INK))
        .child(label(
            format!(
                "Build and upgrade settlements, command knights, cast spells, and be first to the match's goal of {} victory points.",
                rules::goals_text()
            ),
            12.0,
            palette::INK,
        ))
        .child(pill("Read the rules", palette::INK, 0xffffff, true, move || app.rules_open.set(true)))
}

fn connecting(app: AppState) -> impl IntoElement {
    view()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(14.0))
        .child(bold("Connecting…", 18.0, palette::PAPER))
        .child(dynamic(move || app.toast.get().map(|t| label(t, 13.0, palette::GOLD))))
        .child(pill("Leave", 0x8a8070, 0xffffff, true, move || app.leave()))
}

/// What the lobby says about the match it waits to start: who may join and
/// what it is played with, or that nobody knows yet.
fn match_summary(v: &MatchView) -> String {
    if v.random {
        return "Share it: 2–4 players · random settings, revealed when the match starts".to_owned();
    }
    let tiles = v.map_size.tiles();
    let structures: String =
        [(v.shrines, "shrine", "shrines"), (v.mines, "mine", "mines"), (v.mills, "lumber mill", "lumber mills")]
            .iter()
            .filter_map(|&(amount, one, many)| match amount.count(tiles) {
                0 => None,
                1 => Some(format!(" · 1 {one}")),
                n => Some(format!(" · {n} {many}")),
            })
            .collect();
    format!(
        "Share it: 2–4 players · first to {} victory points · {} {} island ({tiles} tiles){structures}",
        v.victory_points,
        v.map_size.label().to_lowercase(),
        v.map_shape.label().to_lowercase(),
    )
}

fn lobby(app: AppState) -> impl IntoElement {
    view()
        .flex_1()
        .child(scroll().flex_1().child(view().p_safe_area().px(px(20.0)).py(px(28.0)).items_center().child(dynamic(
            move || {
                let view_ = app.view.get()?;
                let host = view_.seat == PlayerId(0);
                let count = view_.players.len();
                let players: Vec<View> = view_
                    .players
                    .iter()
                    .enumerate()
                    .map(|(i, n)| {
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(10.0))
                            .child(
                                view()
                                    .size(px(16.0))
                                    .rounded(px(8.0))
                                    .bg(hex(palette::player(PlayerId(i as u8))))
                                    .border_1()
                                    .border_color(hex(palette::INK)),
                            )
                            .child(bold(
                                format!(
                                    "{n}{}{}",
                                    if i == 0 { " · host" } else { "" },
                                    if i == view_.seat.index() { " (you)" } else { "" }
                                ),
                                15.0,
                                palette::INK,
                            ))
                    })
                    .collect();
                let can_start = host && count >= balance::MIN_PLAYERS && !app.busy.get();
                Some(
                    card()
                        .w_full()
                        .max_w(px(hud::MENU_WIDTH))
                        .items_stretch()
                        .child(label("Room code", 12.0, palette::MUTED).self_center())
                        .child({
                            let code = view_.code.clone();
                            view()
                                .self_center()
                                .flex_row()
                                .items_center()
                                .gap(px(10.0))
                                .active(|s| s.opacity(0.7))
                                .on_press(move || app.copy_code(code.clone()))
                                .child(bold(view_.code.clone(), 40.0, palette::INK))
                                .child(dynamic(move || {
                                    let copied = app.code_copied.get();
                                    Some(
                                        view()
                                            .px(px(10.0))
                                            .py(px(5.0))
                                            .rounded(px(12.0))
                                            .bg(hex(if copied { palette::GO } else { palette::INK }))
                                            .child(bold(if copied { "Copied ✓" } else { "Copy" }, 12.0, 0xffffff)),
                                    )
                                }))
                        })
                        .child({
                            let code = view_.code.clone();
                            dynamic(move || {
                                let (copied, code) = (app.link_copied.get(), code.clone());
                                let text = if copied { "Link copied ✓" } else { "Share room link" };
                                Some(pill(
                                    text,
                                    if copied { palette::GO } else { 0x2f6db5 },
                                    0xffffff,
                                    true,
                                    move || app.share_room(code.clone()),
                                ))
                            })
                        })
                        .child(label(match_summary(&view_), 12.0, palette::MUTED).self_center())
                        .child(host.then(|| {
                            let idle = !app.busy.get();
                            let settings = view().gap(px(10.0)).child(choice(
                                "Settings",
                                &[false, true],
                                view_.random,
                                idle,
                                |random| (if random { "Random" } else { "Custom" }).to_owned(),
                                move |random| app.configure(|c| c.random = Some(random)),
                            ));
                            // Random settings stay hidden from everyone, the host too.
                            if view_.random {
                                return settings;
                            }
                            settings
                                .child(choice(
                                    "Victory points",
                                    &balance::VICTORY_POINT_GOALS,
                                    view_.victory_points,
                                    idle,
                                    |goal| goal.to_string(),
                                    move |goal| app.configure(|c| c.victory_points = Some(goal)),
                                ))
                                .child(choice(
                                    "Island size",
                                    &MapSize::ALL,
                                    view_.map_size,
                                    idle,
                                    |size| size.short_label().to_owned(),
                                    move |size| app.configure(|c| c.map_size = Some(size)),
                                ))
                                .child(choice(
                                    "Island shape",
                                    &MapShape::ALL,
                                    view_.map_shape,
                                    idle,
                                    |shape| shape.label().to_owned(),
                                    move |shape| app.configure(|c| c.map_shape = Some(shape)),
                                ))
                                .child(choice(
                                    "Shrines",
                                    &Amount::ALL,
                                    view_.shrines,
                                    idle,
                                    |amount| amount.label().to_owned(),
                                    move |amount| app.configure(|c| c.shrines = Some(amount)),
                                ))
                                .child(choice(
                                    "Mines",
                                    &Amount::ALL,
                                    view_.mines,
                                    idle,
                                    |amount| amount.label().to_owned(),
                                    move |amount| app.configure(|c| c.mines = Some(amount)),
                                ))
                                .child(choice(
                                    "Lumber mills",
                                    &Amount::ALL,
                                    view_.mills,
                                    idle,
                                    |amount| amount.label().to_owned(),
                                    move |amount| app.configure(|c| c.mills = Some(amount)),
                                ))
                        }))
                        .child(view().h(px(1.0)).bg(hexa(palette::INK, 0.12)))
                        .child(players)
                        .child((host && count < balance::MAX_PLAYERS).then(|| {
                            // A press adds one: no chip stays picked.
                            let styles = Style::ALL.map(Some);
                            choice(
                                "Add a bot",
                                &styles,
                                None,
                                !app.busy.get(),
                                |style| style.map_or_else(String::new, |s| s.label().to_owned()),
                                move |style| app.add_bot(style.unwrap_or_default()),
                            )
                        }))
                        .child(if host {
                            pill(
                                if count < balance::MIN_PLAYERS {
                                    "Waiting for players…".to_owned()
                                } else {
                                    format!("Start with {count} players")
                                },
                                palette::GO,
                                0xffffff,
                                can_start,
                                move || app.start_match(),
                            )
                        } else {
                            pill("Waiting for the host to start", 0x8a8070, 0xffffff, false, || {})
                        })
                        .child(if host {
                            // Leaving would strand the guests: the host cancels for everyone.
                            pill("Cancel match", palette::DANGER, 0xffffff, !app.busy.get(), move || app.end_match())
                        } else {
                            pill("Leave", 0x8a8070, 0xffffff, true, move || app.leave())
                        })
                        .child(dynamic(move || app.toast.get().map(|t| label(t, 12.0, palette::DANGER)))),
                )
            },
        ))))
        .child(hud::ended(app))
}
