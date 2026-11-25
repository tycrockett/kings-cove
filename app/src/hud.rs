//! Everything over the board: players and dice at the top, the inventory,
//! hand, action tray and turn button at the bottom, trades, history.

use kings_cove_engine::{
    Action, BuildingKind, Event, Game, Phase, PlayerId, Resource, Resources, SetupPlacing, UnitKind, balance,
};
use nsh_core::prelude::*;

use crate::{
    board,
    palette::{self, hex, hexa},
    state::{AppState, Inspect, Mode, now_ms},
};

// ----- Small pieces -----

pub fn label(s: impl Into<String>, size: f32, color: u32) -> Text {
    text(s.into()).text_size(px(size)).text_color(hex(color))
}

pub fn bold(s: impl Into<String>, size: f32, color: u32) -> Text {
    label(s, size, color).font_weight(FontWeight::BOLD)
}

/// A pill button: `primary` fills it, `enabled` dims it.
pub fn pill(text_: impl Into<String>, fill: u32, fg: u32, enabled: bool, on: impl Fn() + 'static) -> View {
    let v = view()
        .px(px(14.0))
        .py(px(9.0))
        .rounded(px(18.0))
        .items_center()
        .justify_center()
        .bg(hex(fill))
        .opacity(if enabled { 1.0 } else { 0.4 })
        .child(bold(text_, 14.0, fg));
    if enabled { v.active(|s| s.opacity(0.7)).on_press(on) } else { v }
}

fn panel() -> View {
    view()
        .bg(hexa(palette::PAPER, 0.96))
        .rounded(px(14.0))
        .border_1()
        .border_color(hexa(palette::INK, 0.15))
        .shadow_md()
}

fn cost_text(c: &Resources) -> String {
    Resource::ALL
        .iter()
        .filter(|&&r| c.get(r) > 0)
        .map(|&r| format!("{}{}", c.get(r), palette::resource_letter(r)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn resources_text(r: &Resources) -> String {
    r.to_string()
}

fn name(names: &[String], p: PlayerId) -> String {
    names.get(p.index()).cloned().unwrap_or_else(|| format!("Player {}", p.0 + 1))
}

/// One line of history for an event, or `None` for ones not worth a line.
pub fn describe(names: &[String], e: &Event) -> Option<String> {
    let n = |p: PlayerId| name(names, p);
    Some(match e {
        Event::TurnStarted { player, round } => format!("Round {round}: {}'s turn", n(*player)),
        Event::Rolled { player, dice } => format!("{} rolled {} ({}+{})", n(*player), dice.0 + dice.1, dice.0, dice.1),
        Event::Produced { player, resource, amount, .. } => {
            format!("{} collected {amount} {}", n(*player), resource.name())
        }
        Event::ProductionBlocked { .. } => "A robber blocked production".to_owned(),
        Event::BuiltRoad { player, .. } => format!("{} built a road", n(*player)),
        Event::Built { player, kind, .. } => {
            format!("{} placed {} {}", n(*player), article(*kind), kind.name())
        }
        Event::Upgraded { player, to, .. } => format!("{} upgraded to a {}", n(*player), to.name()),
        Event::CardBought { player, kind } => {
            format!("{} bought a {} card", n(*player), kind.name())
        }
        Event::Deployed { player, kind, .. } => {
            format!("{} deployed a {}", n(*player), kind.name())
        }
        Event::UnitMoved { player, kind, .. } => format!("{}'s {} moved", n(*player), kind.name()),
        Event::KnightKilledRobber { player, .. } => {
            format!("{}'s knight slew a robber", n(*player))
        }
        Event::KnightsClashed { attacker, defender, .. } => {
            format!("{}'s and {}'s knights fell together", n(*attacker), n(*defender))
        }
        Event::RamDestroyed { attacker, defender, .. } => {
            format!("{}'s knight broke {}'s ram", n(*attacker), n(*defender))
        }
        Event::BuildingRammed { attacker, defender, was, .. } => match was.downgrade() {
            Some(now) => {
                format!(
                    "{}'s ram cut {}'s {} to {} {}",
                    n(*attacker),
                    n(*defender),
                    was.name(),
                    article(now),
                    now.name()
                )
            }
            None => format!("{}'s ram razed {}'s {}", n(*attacker), n(*defender), was.name()),
        },
        Event::RobberRelocated { player, .. } => format!("{} moved a robber", n(*player)),
        Event::RobberWandered { .. } => "A robber wandered".to_owned(),
        Event::RobberSpawned { .. } => "A robber appeared".to_owned(),
        Event::TradeProposed { from, to, give, want } => format!(
            "{} offers {} for {}{}",
            n(*from),
            resources_text(give),
            resources_text(want),
            to.map(|p| format!(" to {}", n(p))).unwrap_or_default()
        ),
        Event::TradeCancelled { from } => format!("{} withdrew the offer", n(*from)),
        Event::TradeDeclined { player } => format!("{} declined", n(*player)),
        Event::Traded { from, to, give, want } => {
            format!("{} traded {} to {} for {}", n(*from), resources_text(give), n(*to), resources_text(want))
        }
        Event::RoundEnded { .. } => return None,
        Event::Won { player, points } => format!("{} wins with {points} points!", n(*player)),
    })
}

fn article(kind: BuildingKind) -> &'static str {
    if kind == BuildingKind::Outpost { "an" } else { "a" }
}

// ----- Top -----

fn die_face(value: u8, size: f32) -> impl IntoElement {
    let pips: &[(f32, f32)] = match value {
        1 => &[(0.5, 0.5)],
        2 => &[(0.28, 0.28), (0.72, 0.72)],
        3 => &[(0.25, 0.25), (0.5, 0.5), (0.75, 0.75)],
        4 => &[(0.28, 0.28), (0.72, 0.28), (0.28, 0.72), (0.72, 0.72)],
        5 => &[(0.25, 0.25), (0.75, 0.25), (0.5, 0.5), (0.25, 0.75), (0.75, 0.75)],
        _ => &[(0.28, 0.22), (0.72, 0.22), (0.28, 0.5), (0.72, 0.5), (0.28, 0.78), (0.72, 0.78)],
    };
    let pips = pips.to_vec();
    canvas(move || {
        let mut p = Painter::new()
            .begin_path()
            .round_rect(bounds(point(px(1.0), px(1.0)), self::size(px(size - 2.0), px(size - 2.0))), px(size * 0.2))
            .fill_color(hex(0xfbf8f0))
            .fill()
            .stroke_color(hex(palette::INK))
            .line_width(1.5)
            .stroke()
            .fill_color(hex(palette::INK));
        for &(x, y) in &pips {
            p = p.begin_path().arc(x * size, y * size, size * 0.09, 0.0, std::f32::consts::TAU).fill();
        }
        p
    })
    .size(px(size))
}

fn dice(app: AppState) -> impl IntoElement {
    dynamic(move || {
        let dice = app.view.with(|v| v.as_ref().and_then(|v| v.game.as_ref().and_then(|g| g.dice)));
        let tumbling = app.dice_until.get() > app.clock.get().max(now_ms() - 1.0);
        let (a, b) = if tumbling {
            let t = (app.clock.get() / 70.0) as u64;
            ((t * 7 % 6) as u8 + 1, (t * 13 % 6) as u8 + 1)
        } else {
            dice?
        };
        Some(
            view()
                .flex_row()
                .gap(px(4.0))
                .items_center()
                .child(die_face(a, 26.0))
                .child(die_face(b, 26.0))
                .child(if tumbling { None } else { Some(bold(format!("{}", a + b), 15.0, 0xffffff).ml(px(2.0))) }),
        )
    })
}

fn players_bar(app: AppState) -> impl IntoElement {
    dynamic(move || {
        let mv = app.view.get()?;
        let g = mv.game.as_ref()?;
        let chips: Vec<View> = g
            .players
            .iter()
            .map(|p| {
                let active = g.current == p.id && !matches!(g.phase, Phase::Finished { .. });
                let me = p.id == mv.seat;
                let cards = p.cards.knights + p.cards.rams;
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(5.0))
                    .px(px(8.0))
                    .py(px(5.0))
                    .rounded(px(12.0))
                    .bg(hexa(palette::PAPER, if active { 0.98 } else { 0.78 }))
                    .border_2()
                    .border_color(hex(if active { palette::GOLD } else { palette::PAPER_DIM }))
                    .child(
                        view()
                            .size(px(12.0))
                            .rounded(px(6.0))
                            .bg(hex(palette::player(p.id)))
                            .border_1()
                            .border_color(hex(palette::INK)),
                    )
                    .child(
                        view()
                            .child(
                                bold(if me { format!("{} (you)", p.name) } else { p.name.clone() }, 11.0, palette::INK)
                                    .number_of_lines(1)
                                    .max_w(px(74.0)),
                            )
                            .child(label(
                                format!(
                                    "{} VP · {} res{}",
                                    g.points(p.id),
                                    p.resources.total(),
                                    if cards > 0 { format!(" · {cards} cards") } else { String::new() }
                                ),
                                10.0,
                                palette::MUTED,
                            )),
                    )
            })
            .collect();
        Some(view().flex_row().flex_wrap().gap(px(5.0)).flex_1().child(chips))
    })
}

fn status_text(app: AppState, g: &Game, seat: PlayerId, names: &[String]) -> String {
    let who = name(names, g.current);
    match g.phase {
        Phase::Finished { winner } => format!("{} wins!", name(names, winner)),
        Phase::Setup { step, placing } if g.current == seat => match placing {
            SetupPlacing::Building => {
                format!("Place your starting {}", g.setup_building_kind(step).name())
            }
            SetupPlacing::Road { .. } => "Place a road beside it".to_owned(),
        },
        Phase::Setup { .. } => format!("{who} is placing…"),
        Phase::Playing if g.current != seat => format!("{who}'s turn"),
        Phase::Playing if g.dice.is_none() => "Your turn: roll the dice".to_owned(),
        Phase::Playing => match app.mode.get() {
            Mode::Road => "Tap a glowing edge to build a road".to_owned(),
            Mode::Outpost => "Tap a spot on your roads for an outpost".to_owned(),
            Mode::UpgradeTown => "Tap an outpost to make it a town".to_owned(),
            Mode::UpgradeCity => "Tap a town to make it a city".to_owned(),
            Mode::Deploy(k) => format!("Tap a tile beside your town or city for the {}", k.name()),
            Mode::Unit(u) => match g.unit(u).map(|u| u.kind) {
                Some(UnitKind::Knight) => "Gold: move · Red: attack robbers, knights, rams".to_owned(),
                _ => "Gold: move · Red: ram a building".to_owned(),
            },
            Mode::Robber(_) => "Tap any free tile to move the robber".to_owned(),
            Mode::Idle if g.robber_relocation => "You rolled 7: tap a robber to move it".to_owned(),
            Mode::Idle => "Your turn: build, trade, or command units".to_owned(),
        },
    }
}

fn inspect_text(g: &Game, names: &[String], i: Inspect) -> String {
    match i {
        Inspect::Tile(t) => {
            let tile = g.board.tile(t);
            let mut s = format!("{} · {}", tile.resource.name(), tile.number);
            if let Some(u) = g.unit_at(t) {
                s += &format!(
                    " · {}'s {}{}",
                    name(names, u.owner),
                    u.kind.name(),
                    if u.ready { "" } else { " (resting)" }
                );
            }
            if g.robber_at(t).is_some() {
                s += " · robber (blocked)";
            }
            s
        }
        Inspect::Vertex(v) => match g.building(v) {
            Some(b) => format!(
                "{}'s {} · {} VP · yields {} per roll",
                name(names, b.owner),
                b.kind.name(),
                b.kind.points(),
                b.kind.yield_per_tile()
            ),
            None => "Empty spot".to_owned(),
        },
        Inspect::Edge(e) => match g.road(e) {
            Some(p) => format!("{}'s road", name(names, p)),
            None => "Open edge".to_owned(),
        },
    }
}

fn top(app: AppState) -> impl IntoElement {
    view()
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .right(px(0.0))
        .pt_safe_area_plus(px(8.0))
        .px(px(8.0))
        .gap(px(6.0))
        .pointer_events(PointerEvents::BoxNone)
        .child(
            view().flex_row().items_start().gap(px(6.0)).child(players_bar(app)).child(
                view()
                    .items_end()
                    .gap(px(4.0))
                    .child(dice(app))
                    .child(dynamic(move || {
                        let round = app.view.with(|v| v.as_ref().and_then(|v| v.game.as_ref().map(|g| g.round)))?;
                        let online = app.connected.get();
                        Some(label(
                            if !online {
                                "reconnecting…".to_owned()
                            } else if round == 0 {
                                "setup".to_owned()
                            } else {
                                format!("round {round}")
                            },
                            11.0,
                            if online { 0xe7dfcb } else { palette::GOLD },
                        ))
                    }))
                    .child(
                        view()
                            .px(px(10.0))
                            .py(px(4.0))
                            .rounded(px(10.0))
                            .bg(hexa(palette::PAPER, 0.85))
                            .active(|s| s.opacity(0.6))
                            .on_press(move || app.fit_request.update(|n| *n += 1))
                            .child(bold("Recenter", 11.0, palette::INK)),
                    ),
            ),
        )
        .child(dynamic(move || {
            let view = app.view.get()?;
            let g = view.game.as_ref()?;
            let (main, color) = match app.inspect.get() {
                Some(i) => (inspect_text(g, &view.players, i), palette::INK),
                None => (status_text(app, g, view.seat, &view.players), palette::INK),
            };
            let last = g.events.iter().rev().find_map(|e| describe(&view.players, &e.event));
            Some(
                panel()
                    .self_start()
                    .px(px(12.0))
                    .py(px(7.0))
                    .gap(px(2.0))
                    .active(|s| s.opacity(0.8))
                    .on_press(move || app.history_open.update(|o| *o = !*o))
                    .child(bold(main, 13.0, color))
                    .child(last.map(|l| label(l, 11.0, palette::MUTED).number_of_lines(1))),
            )
        }))
        .child(dynamic(move || {
            if !app.history_open.get() {
                return None;
            }
            let view = app.view.get()?;
            let g = view.game.as_ref()?;
            let lines: Vec<Text> = g
                .events
                .iter()
                .rev()
                .filter_map(|e| describe(&view.players, &e.event))
                .take(14)
                .map(|l| label(l, 12.0, palette::INK).number_of_lines(1))
                .collect();
            Some(
                panel()
                    .px(px(12.0))
                    .py(px(8.0))
                    .gap(px(3.0))
                    .on_press(move || app.history_open.set(false))
                    .child(bold("History", 12.0, palette::MUTED))
                    .child(lines),
            )
        }))
}

// ----- Bottom -----

fn inventory(app: AppState, g: &Game, seat: PlayerId) -> View {
    let me = &g.players[seat.index()];
    let can_deploy = g.is_active(seat) && g.dice.is_some();
    let resources = Resource::ALL.map(|r| {
        view()
            .flex_row()
            .items_center()
            .gap(px(5.0))
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(10.0))
            .bg(hexa(palette::PAPER, 0.95))
            .child(
                view()
                    .size(px(16.0))
                    .rounded(px(4.0))
                    .bg(hex(palette::resource(r)))
                    .border_1()
                    .border_color(hex(palette::shade(palette::resource(r), 0.6))),
            )
            .child(bold(me.resources.get(r).to_string(), 15.0, palette::INK))
            .child(label(r.name(), 9.0, palette::MUTED))
    });
    let card = |kind: UnitKind, count: u32| {
        let mode = Mode::Deploy(kind);
        let selected = app.mode.get_untracked() == mode;
        let enabled = can_deploy && count > 0;
        let v = view()
            .flex_row()
            .items_center()
            .gap(px(4.0))
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(10.0))
            .bg(hex(if selected { palette::GOLD } else { 0x5a4a3a }))
            .opacity(if count > 0 { 1.0 } else { 0.45 })
            .child(bold(
                format!("{} ×{count}", if kind == UnitKind::Knight { "Knight" } else { "Ram" }),
                12.0,
                if selected { palette::INK } else { 0xffffff },
            ));
        if enabled {
            v.active(|s| s.opacity(0.7)).on_press(move || {
                app.inspect.set(None);
                app.mode.set(if app.mode.get_untracked() == mode { Mode::Idle } else { mode });
            })
        } else {
            v
        }
    };
    view()
        .flex_row()
        .flex_wrap()
        .gap(px(5.0))
        .child(resources.to_vec())
        .child(card(UnitKind::Knight, me.cards.knights))
        .child(card(UnitKind::Ram, me.cards.rams))
}

struct TrayItem {
    title: &'static str,
    cost: Resources,
    mode: Option<Mode>,
    action: Option<Action>,
}

fn tray(app: AppState, g: &Game, seat: PlayerId) -> View {
    let my_move = g.is_active(seat) && g.dice.is_some();
    let items = [
        TrayItem { title: "Road", cost: balance::ROAD_COST, mode: Some(Mode::Road), action: None },
        TrayItem { title: "Outpost", cost: balance::OUTPOST_COST, mode: Some(Mode::Outpost), action: None },
        TrayItem { title: "Town ↑", cost: balance::TOWN_UPGRADE_COST, mode: Some(Mode::UpgradeTown), action: None },
        TrayItem { title: "City ↑", cost: balance::CITY_UPGRADE_COST, mode: Some(Mode::UpgradeCity), action: None },
        TrayItem {
            title: "Knight card",
            cost: balance::KNIGHT_CARD_COST,
            mode: None,
            action: Some(Action::BuyCard(UnitKind::Knight)),
        },
        TrayItem {
            title: "Ram card",
            cost: balance::RAM_CARD_COST,
            mode: None,
            action: Some(Action::BuyCard(UnitKind::Ram)),
        },
    ];
    let current = app.mode.get_untracked();
    let buttons: Vec<View> = items
        .into_iter()
        .map(|item| {
            let enabled = my_move
                && match (item.mode, &item.action) {
                    (Some(mode), _) => !board::options(g, seat, mode).is_empty(),
                    (None, Some(Action::BuyCard(k))) => g.can_buy_card(seat, *k).is_ok(),
                    _ => false,
                };
            let selected = item.mode.is_some_and(|m| m == current);
            let v = view()
                .items_center()
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(10.0))
                .min_w(px(62.0))
                .bg(hex(if selected { palette::GOLD } else { palette::PAPER }))
                .opacity(if enabled || selected { 1.0 } else { 0.45 })
                .child(bold(item.title, 12.0, palette::INK))
                .child(label(cost_text(&item.cost), 10.0, palette::MUTED));
            if !(enabled || selected) {
                return v;
            }
            v.active(|s| s.opacity(0.7)).on_press(move || {
                app.inspect.set(None);
                match (item.mode, item.action.clone()) {
                    (Some(mode), _) => app.mode.set(if app.mode.get_untracked() == mode { Mode::Idle } else { mode }),
                    (None, Some(action)) => app.send(action),
                    _ => {}
                }
            })
        })
        .collect();
    let trade_enabled = my_move && g.trade.is_none();
    let trade = view()
        .items_center()
        .justify_center()
        .px(px(12.0))
        .py(px(6.0))
        .rounded(px(10.0))
        .bg(hex(if app.trade_open.get_untracked() { palette::GOLD } else { 0x2f6db5 }))
        .opacity(if trade_enabled { 1.0 } else { 0.45 })
        .child(bold("Trade", 12.0, if app.trade_open.get_untracked() { palette::INK } else { 0xffffff }))
        .child(label("offer", 10.0, if app.trade_open.get_untracked() { palette::INK } else { 0xdfe8f5 }));
    let trade = if trade_enabled {
        trade.active(|s| s.opacity(0.7)).on_press(move || app.trade_open.update(|o| *o = !*o))
    } else {
        trade
    };
    view().child(scroll().horizontal().child(view().flex_row().gap(px(6.0)).child(buttons).child(trade)))
}

fn primary(app: AppState, g: &Game, seat: PlayerId, names: &[String]) -> View {
    let busy = app.busy.get();
    match g.phase {
        Phase::Playing if g.current == seat && g.dice.is_none() => {
            big_button("Roll dice", palette::GOLD, palette::INK, !busy, move || app.send(Action::Roll))
        }
        Phase::Playing if g.current == seat => big_button("End turn", palette::GO, 0xffffff, !busy, move || {
            app.mode.set(Mode::Idle);
            app.trade_open.set(false);
            app.send(Action::EndTurn)
        }),
        Phase::Playing => {
            big_button(format!("Waiting for {}", name(names, g.current)), 0x55606a, 0xffffff, false, || {})
        }
        _ => view(),
    }
}

fn big_button(title: impl Into<String>, fill: u32, fg: u32, enabled: bool, on: impl Fn() + 'static) -> View {
    let v = view()
        .h(px(50.0))
        .rounded(px(25.0))
        .items_center()
        .justify_center()
        .bg(hex(fill))
        .shadow_md()
        .opacity(if enabled { 1.0 } else { 0.6 })
        .child(bold(title, 17.0, fg));
    if enabled { v.active(|s| s.opacity(0.75).scale(0.98)).on_press(on) } else { v }
}

/// The open offer: answer it, or watch it as its maker.
fn trade_offer(app: AppState, g: &Game, seat: PlayerId, names: &[String]) -> Option<View> {
    let offer = g.trade.as_ref()?;
    let from = name(names, offer.from);
    if offer.from == seat {
        return Some(
            panel()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(12.0))
                .py(px(8.0))
                .child(
                    view()
                        .flex_1()
                        .child(bold(format!("Offering {} for {}", offer.give, offer.want), 13.0, palette::INK))
                        .child(label(
                            if offer.declined.is_empty() {
                                "Waiting for answers…".to_owned()
                            } else {
                                format!(
                                    "Declined by {}",
                                    offer.declined.iter().map(|&p| name(names, p)).collect::<Vec<_>>().join(", ")
                                )
                            },
                            11.0,
                            palette::MUTED,
                        )),
                )
                .child(pill("Withdraw", 0x8a8070, 0xffffff, true, move || app.send(Action::CancelTrade))),
        );
    }
    let for_me = offer.to.is_none_or(|p| p == seat) && !offer.declined.contains(&seat);
    if !for_me {
        return None;
    }
    let affordable = g.players[seat.index()].resources.covers(&offer.want);
    Some(
        panel()
            .px(px(12.0))
            .py(px(10.0))
            .gap(px(8.0))
            .border_2()
            .border_color(hex(palette::GOLD))
            .child(bold(format!("{from} offers {} for your {}", offer.give, offer.want), 14.0, palette::INK))
            .child(
                view()
                    .flex_row()
                    .gap(px(8.0))
                    .child(pill("Accept", palette::GO, 0xffffff, affordable, move || app.send(Action::AcceptTrade)))
                    .child(pill("Decline", 0x8a8070, 0xffffff, true, move || app.send(Action::DeclineTrade))),
            ),
    )
}

/// Composing an offer: what to give, what to get, and to whom.
fn trade_composer(app: AppState) -> impl IntoElement {
    let give = signal(Resources::ZERO);
    let want = signal(Resources::ZERO);
    let to = signal(None::<PlayerId>);
    let stepper_row = move |title: &'static str, side: Signal<Resources>, limit: Option<Resources>| {
        dynamic(move || {
            let current = side.get();
            let cells: Vec<View> = Resource::ALL
                .iter()
                .map(|&r| {
                    let n = current.get(r);
                    let max = limit.map_or(9, |l| l.get(r));
                    view()
                        .flex_1()
                        .items_center()
                        .gap(px(2.0))
                        .child(view().size(px(14.0)).rounded(px(3.0)).bg(hex(palette::resource(r))))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(3.0))
                                .child(step_button("−", n > 0, move || side.update(|s| *s.get_mut(r) -= 1)))
                                .child(bold(n.to_string(), 14.0, palette::INK).min_w(px(14.0)).text_center())
                                .child(step_button("+", n < max, move || side.update(|s| *s.get_mut(r) += 1))),
                        )
                })
                .collect();
            view().gap(px(3.0)).child(label(title, 11.0, palette::MUTED)).child(view().flex_row().child(cells))
        })
    };
    dynamic(move || {
        if !app.trade_open.get() {
            return None;
        }
        let view_ = app.view.get()?;
        let g = view_.game.as_ref()?;
        let seat = view_.seat;
        if !g.is_active(seat) || g.dice.is_none() || g.trade.is_some() {
            return None;
        }
        let mine = g.players[seat.index()].resources;
        let targets: Vec<View> = std::iter::once(None)
            .chain(g.players.iter().filter(|p| p.id != seat).map(|p| Some(p.id)))
            .map(|target| {
                let title = target.map_or("Anyone".to_owned(), |p| name(&view_.players, p));
                let selected = to.get() == target;
                view()
                    .px(px(10.0))
                    .py(px(5.0))
                    .rounded(px(12.0))
                    .bg(hex(if selected { palette::INK } else { palette::PAPER_DIM }))
                    .on_press(move || to.set(target))
                    .child(bold(title, 12.0, if selected { 0xffffff } else { palette::INK }))
            })
            .collect();
        let ready = !give.get().is_empty() && !want.get().is_empty() && mine.covers(&give.get());
        Some(
            panel()
                .px(px(12.0))
                .py(px(10.0))
                .gap(px(8.0))
                .child(stepper_row("You give", give, Some(mine)))
                .child(stepper_row("You get", want, None))
                .child(view().flex_row().flex_wrap().gap(px(5.0)).child(targets))
                .child(
                    view()
                        .flex_row()
                        .gap(px(8.0))
                        .child(pill("Propose", palette::GO, 0xffffff, ready, move || {
                            app.send(Action::ProposeTrade {
                                to: to.get_untracked(),
                                give: give.get_untracked(),
                                want: want.get_untracked(),
                            });
                            give.set(Resources::ZERO);
                            want.set(Resources::ZERO);
                            app.trade_open.set(false);
                        }))
                        .child(pill("Close", 0x8a8070, 0xffffff, true, move || app.trade_open.set(false))),
                ),
        )
    })
}

fn step_button(t: &'static str, enabled: bool, on: impl Fn() + 'static) -> View {
    let v = view()
        .size(px(24.0))
        .rounded(px(12.0))
        .items_center()
        .justify_center()
        .bg(hex(palette::PAPER_DIM))
        .opacity(if enabled { 1.0 } else { 0.35 })
        .child(bold(t, 14.0, palette::INK));
    if enabled { v.active(|s| s.opacity(0.6)).on_press(on) } else { v }
}

fn bottom(app: AppState) -> impl IntoElement {
    view()
        .absolute()
        .left(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .px(px(8.0))
        .pb_safe_area_plus(px(10.0))
        .gap(px(6.0))
        .pointer_events(PointerEvents::BoxNone)
        .child(dynamic(move || {
            app.toast.get().map(|t| {
                view()
                    .self_center()
                    .px(px(14.0))
                    .py(px(8.0))
                    .rounded(px(16.0))
                    .bg(hexa(0x3a1f1c, 0.92))
                    .child(bold(t, 13.0, 0xffffff))
            })
        }))
        .child(trade_composer(app))
        .child(dynamic(move || {
            let view_ = app.view.get()?;
            let g = view_.game.as_ref()?;
            let _ = app.mode.get();
            let _ = app.trade_open.get();
            let seat = view_.seat;
            let setup = matches!(g.phase, Phase::Setup { .. });
            Some(
                view()
                    .gap(px(6.0))
                    .child(trade_offer(app, g, seat, &view_.players))
                    .child(inventory(app, g, seat))
                    .child(if setup || !g.is_active(seat) || g.dice.is_none() {
                        None
                    } else {
                        Some(tray(app, g, seat))
                    })
                    .child(primary(app, g, seat, &view_.players)),
            )
        }))
}

fn winner(app: AppState) -> impl IntoElement {
    dynamic(move || {
        let view_ = app.view.get()?;
        let g = view_.game.as_ref()?;
        let w = g.winner()?;
        let me = w == view_.seat;
        Some(
            view().absolute().inset_0().items_center().justify_center().bg(hexa(0x000000, 0.45)).child(
                panel()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(28.0))
                    .py(px(24.0))
                    .child(
                        view()
                            .size(px(26.0))
                            .rounded(px(13.0))
                            .bg(hex(palette::player(w)))
                            .border_2()
                            .border_color(hex(palette::INK)),
                    )
                    .child(bold(
                        if me { "You win!".to_owned() } else { format!("{} wins", name(&view_.players, w)) },
                        26.0,
                        palette::INK,
                    ))
                    .child(label(
                        g.players
                            .iter()
                            .map(|p| format!("{} {}", p.name, g.points(p.id)))
                            .collect::<Vec<_>>()
                            .join(" · "),
                        13.0,
                        palette::MUTED,
                    ))
                    .child(pill("Leave match", palette::INK, 0xffffff, true, move || app.leave())),
            ),
        )
    })
}

/// The game screen: the board under its HUD.
pub fn game_screen(app: AppState) -> impl IntoElement {
    // Leave a selection that no longer applies (the unit died, the 7 passed).
    effect(move || {
        let Some(view_) = app.view.get() else { return };
        let Some(g) = view_.game.as_ref() else { return };
        let mode = untracked(move || app.mode.get());
        let stale = match mode {
            Mode::Unit(u) => g.unit(u).is_none_or(|u| !u.ready),
            Mode::Robber(_) => !g.robber_relocation,
            Mode::Idle => false,
            _ => !g.is_active(view_.seat) || g.dice.is_none(),
        };
        if stale {
            app.mode.set(Mode::Idle);
        }
    });
    view()
        .flex_1()
        .bg(hex(palette::TABLE))
        .child(board::board_view(app))
        .child(top(app))
        .child(bottom(app))
        .child(winner(app))
}
