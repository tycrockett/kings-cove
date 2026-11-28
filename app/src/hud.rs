//! Everything over the board: players and dice at the top, the dock with
//! the inventory, the build tray and the turn button at the bottom, trades,
//! history and the winner.

use kings_cove_engine::{
    Action, Blockade, BuildingKind, Event, Game, Phase, PlayerId, Resource, Resources, SetupPlacing, Spell, Target,
    UnitKind, balance,
};
use kings_cove_protocol::{Reaction, SentReaction};
use nsh_core::{LayoutCurve, LayoutTransition, prelude::*};

use crate::{
    art::{self, Glyph},
    board, fx,
    palette::{self, hex, hexa},
    rules::{self, Item},
    state::{AppState, Hud, Inspect, Menu, Mode, now_ms},
};

// ----- Small pieces -----

pub fn label(s: impl Into<String>, size: f32, color: u32) -> Text {
    text(s.into()).text_size(px(size)).text_color(hex(color))
}

pub fn bold(s: impl Into<String>, size: f32, color: u32) -> Text {
    label(s, size, color).font_weight(FontWeight::BOLD)
}

/// A pill button: `fill` colors it, `enabled` dims it.
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
    if enabled { v.active(|s| s.opacity(0.7).scale(0.97)).on_press(on) } else { v }
}

/// The widest a menu card or column grows: phones fill their width, wider
/// screens keep a phone-sized column in the middle.
pub const MENU_WIDTH: f32 = 440.0;
/// The widest the rulebook grows.
const RULEBOOK_WIDTH: f32 = 640.0;

fn panel() -> View {
    view()
        .bg(hexa(palette::PAPER, 0.97))
        .rounded(px(14.0))
        .border_1()
        .border_color(hexa(palette::INK, 0.15))
        .shadow_md()
}

/// A resource's badge, `s` points across.
pub fn resource_icon(r: Resource, s: f32) -> impl IntoElement {
    canvas(move || art::resource_icon(Painter::new(), r, (s / 2.0, s / 2.0), s - 1.5, 1.0)).size(px(s))
}

fn glyph_icon(g: Glyph, color: u32, w: f32, h: f32) -> impl IntoElement {
    canvas(move || art::glyph(Painter::new(), g, color, w, h)).w(px(w)).h(px(h))
}

fn star_icon(s: f32) -> impl IntoElement {
    canvas(move || {
        art::star(Painter::new(), (s / 2.0, s / 2.0 + 0.5), s / 2.0)
            .fill_color(hex(palette::GOLD))
            .fill()
            .stroke_color(hex(0x8a5a10))
            .line_width(1.0)
            .stroke()
    })
    .size(px(s))
}

/// A drop of mana: full when a spell can be cast, hollow while it refills.
fn mana_icon(s: f32, full: bool) -> impl IntoElement {
    canvas(move || {
        let (w, h) = (s, s);
        let drop = Painter::new()
            .begin_path()
            .move_to(w * 0.5, h * 0.06)
            .bezier_curve_to(
                point(px(w * 0.5), px(h * 0.06)),
                point(px(w * 0.88), px(h * 0.52)),
                point(px(w * 0.88), px(h * 0.66)),
            )
            .arc(w * 0.5, h * 0.66, w * 0.38, 0.0, std::f32::consts::PI)
            .bezier_curve_to(
                point(px(w * 0.12), px(h * 0.52)),
                point(px(w * 0.5), px(h * 0.06)),
                point(px(w * 0.5), px(h * 0.06)),
            )
            .close_path();
        let drop = if full { drop.fill_color(hex(0x4a8fe0)).fill() } else { drop.fill_color(hex(0xe3ddd0)).fill() };
        drop.stroke_color(hex(0x22508a)).line_width(1.0).stroke()
    })
    .size(px(s))
}

/// Two stacked cards: a hand of resources.
fn cards_icon(s: f32) -> impl IntoElement {
    canvas(move || {
        let card = |p: Painter, x: f32, color: u32| {
            art::rrect(p, (x, s / 2.0), s * 0.55, s * 0.8, 2.0)
                .fill_color(hex(color))
                .fill()
                .stroke_color(hex(palette::INK))
                .line_width(1.0)
                .stroke()
        };
        card(card(Painter::new(), s * 0.38, 0xd9cfb8), s * 0.62, 0xfffaf0)
    })
    .size(px(s))
}

fn die_face(value: u8, s: f32) -> impl IntoElement {
    canvas(move || art::die(Painter::new(), (s / 2.0, s / 2.0), s - 2.0, value, 0.0, 1.2)).size(px(s))
}

/// Resources as icons with counts; counts the player lacks (`have`) in red.
fn cost_row(cost: &Resources, have: Option<&Resources>, icon: f32, font: f32) -> View {
    let cells: Vec<View> = Resource::ALL
        .iter()
        .filter(|&&r| cost.get(r) > 0)
        .map(|&r| {
            let short = have.is_some_and(|h| h.get(r) < cost.get(r));
            view().flex_row().items_center().gap(px(1.0)).child(resource_icon(r, icon)).child(bold(
                cost.get(r).to_string(),
                font,
                if short { palette::DANGER } else { palette::INK },
            ))
        })
        .collect();
    view().flex_row().items_center().gap(px(4.0)).child(cells)
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
        Event::ProductionBlocked { .. } => "A bandit blocked production".to_owned(),
        Event::BuiltRoad { player, .. } => format!("{} built a road", n(*player)),
        Event::Built { player, kind, .. } => {
            format!("{} placed {} {}", n(*player), article(*kind), kind.name())
        }
        Event::Upgraded { player, to, .. } => format!("{} upgraded to a {}", n(*player), to.name()),
        Event::CardBought { player, kind } => {
            format!("{} bought a {} token", n(*player), kind.name())
        }
        Event::Deployed { player, kind, .. } => {
            format!("{} deployed a {}", n(*player), kind.name())
        }
        Event::UnitMoved { player, kind, .. } => format!("{}'s {} moved", n(*player), kind.name()),
        Event::KnightKilledRobber { player, .. } => {
            format!("{}'s knight fell bringing down a bandit", n(*player))
        }
        Event::KnightsClashed { attacker, defender, .. } => {
            format!("{}'s and {}'s knights fell together", n(*attacker), n(*defender))
        }
        Event::SpellCast { player, spell, .. } => format!("{} cast {}", n(*player), spell.name()),
        Event::ProductionStopped { spell, .. } => format!("{} stopped a harvest", rules::spell_title(*spell)),
        Event::WindfallPaid { player, gained } => format!("{} collected a windfall: {gained}", n(*player)),
        Event::ItemAppeared { .. } => "Something appeared on the island".to_owned(),
        Event::ItemFound { player, kind, count, gained, scroll, .. } => {
            format!("{}'s knight found {}", n(*player), rules::item_found(*kind, *count, gained, *scroll))
        }
        Event::ItemVanished { .. } => "An item vanished".to_owned(),
        Event::ItemStolen { .. } => "A bandit took an item".to_owned(),
        Event::RelicDropped { player, count, .. } => {
            format!("{}'s fallen knight dropped {count} relic{}", n(*player), if *count == 1 { "" } else { "s" })
        }
        Event::SpellFaded { spell, .. } => {
            format!("The {} faded", spell.name())
        }
        Event::RobberRelocated { player, .. } => format!("{} moved a bandit", n(*player)),
        Event::RobberWandered { .. } => "A bandit wandered".to_owned(),
        Event::RobberSpawned { .. } => "A bandit appeared".to_owned(),
        Event::RobberPlaced { player, .. } => format!("{} placed a bandit", n(*player)),
        Event::StructureTaken { player, kind, .. } => {
            format!(
                "{}'s knight took a {}: {} while it stands there",
                n(*player),
                kind.name(),
                rules::structure_gain(*kind)
            )
        }
        Event::StructureSeized { kind, .. } => format!("A bandit seized a {}", kind.name()),
        Event::StructurePaid { player, kind, resource, amount, .. } => {
            format!("{} collected {amount} {} from a {}", n(*player), resource.name(), kind.name())
        }
        Event::WardStruck { caster, knight, .. } => match knight {
            Some(owner) => format!("{}'s ward felled {}'s knight", n(*caster), n(*owner)),
            None => format!("{}'s ward felled a bandit", n(*caster)),
        },
        Event::BlockadeLaid { player, .. } => format!("{}'s knight blockaded a tile", n(*player)),
        Event::BlockadeLifting { player, free_after, .. } => {
            format!("{}'s knight is packing up its blockade (free after round {free_after})", n(*player))
        }
        Event::BlockadeEnded { player, .. } => format!("{}'s blockade lifted", n(*player)),
        Event::ProductionBlockaded { player, .. } => format!("{}'s blockade stopped a harvest", n(*player)),
        Event::RingPaid { player, resource, amount, .. } => {
            format!("{} collected {amount} {} from a ringed tile", n(*player), resource.name())
        }
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
        Event::BankTraded { player, give, get } => format!(
            "{} traded {} {} to the bank for 1 {}",
            n(*player),
            balance::BANK_TRADE_RATE,
            give.name(),
            get.name()
        ),
        Event::RoundEnded { .. } => return None,
        Event::Won { player, points } => format!("{} wins with {points} points!", n(*player)),
    })
}

fn article(kind: BuildingKind) -> &'static str {
    if kind == BuildingKind::Outpost { "an" } else { "a" }
}

// ----- Top -----

/// The corner the rolled dice fly into; empty while they are in the air.
fn dice(app: AppState) -> impl IntoElement {
    let shown = computed(move || {
        if app.dice_hidden.get() {
            return None;
        }
        app.view.with(|v| v.as_ref().and_then(|v| v.game.as_ref().and_then(|g| g.dice)))
    });
    view().flex_row().justify_end().w(px(fx::SLOT_WIDTH)).h(px(fx::SLOT_DIE)).child(dynamic(move || {
        let (a, b) = shown.get()?;
        let sum = a + b;
        let fill = if sum == 7 { palette::DANGER } else { palette::GOLD };
        Some(
            view()
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .child(die_face(a, fx::SLOT_DIE))
                .child(die_face(b, fx::SLOT_DIE))
                .child(
                    view()
                        .size(px(24.0))
                        .rounded(px(12.0))
                        .items_center()
                        .justify_center()
                        .bg(hex(fill))
                        .border_2()
                        .border_color(hex(0xffffff))
                        .bind_motion(&motion().entering(MotionPreset::PopIn))
                        .child(bold(sum.to_string(), 12.0, palette::ink_on(fill))),
                ),
        )
    }))
}

/// A player's avatar: their initial on their color, ringed in gold while it
/// is their turn.
fn avatar(id: PlayerId, name: &str, size: f32, active: bool) -> View {
    let color = palette::player(id);
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    view()
        .size(px(size))
        .rounded(px(size / 2.0))
        .items_center()
        .justify_center()
        .bg(hex(color))
        .border_2()
        .border_color(hex(if active { palette::GOLD } else { 0xffffff }))
        .child(bold(initial, size * 0.46, palette::ink_on(color)))
}

/// A player waiting for their turn, as the queue shows them.
#[derive(Clone, PartialEq)]
struct Waiting {
    id: PlayerId,
    name: String,
    points: u32,
}

/// Under the active player's chip: the players whose turns come next, in
/// order, each avatar over their points on a dark translucent card. As the
/// turn passes they move up, and the player who just went joins the end.
fn turn_queue(app: AppState) -> impl IntoElement {
    let queue = computed(move || {
        app.view.with(|v| {
            let Some(g) = v.as_ref().and_then(|v| v.game.as_ref()) else { return Vec::new() };
            let n = g.players.len();
            (1..n)
                .map(|k| &g.players[(g.current.index() + k) % n])
                .map(|p| Waiting { id: p.id, name: p.name.clone(), points: g.points(p.id) })
                .collect()
        })
    });
    let column = element_ref();
    let turn = stored(None::<PlayerId>);
    effect(move || {
        let current = app.view.with(|v| v.as_ref().and_then(|v| v.game.as_ref()).map(|g| g.current));
        let before = turn.replace(current);
        if before.is_some() && before != current {
            column.animate_layout(LayoutTransition::new(LayoutCurve::EaseInEaseOut, 350.0));
        }
    });
    let card = |w: ReadSignal<Waiting>| {
        dynamic(move || {
            let w = w.get();
            let points = view().flex_row().items_center().gap(px(2.0)).child(star_icon(10.0)).child(bold(
                w.points.to_string(),
                10.0,
                0xffffff,
            ));
            Some(
                view()
                    .items_center()
                    .gap(px(2.0))
                    .px(px(4.0))
                    .pt(px(4.0))
                    .pb(px(2.0))
                    .rounded(px(12.0))
                    .bg(hexa(0x000000, 0.45))
                    .child(avatar(w.id, &w.name, 24.0, false))
                    .child(points),
            )
        })
    };
    // Out of the column's flow, like the panel of everyone, so the top bar
    // stays as tall as its first row and the board fits under that.
    view()
        .element_ref(&column)
        .absolute()
        .top(px(46.0))
        .left(px(0.0))
        .gap(px(6.0))
        .items_start()
        .pointer_events(PointerEvents::None)
        .child(each(
            queue,
            |w: &Waiting| w.id,
            move |w, _| {
                let id = w.get_untracked().id;
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .bind_motion(&motion().entering(MotionPreset::FadeInUp))
                    .child(card(w))
                    .child(reaction_bubbles(app, move || Some(id)))
            },
        ))
}

/// A player's chip: color and initial, name, and their points, resources,
/// knight tokens and mana.
fn player_chip(g: &Game, p: &kings_cove_engine::Player, me: bool, active: bool) -> View {
    let cards = p.cards.knights;
    let color = palette::player(p.id);
    let stats = view()
        .flex_row()
        .items_center()
        .gap(px(3.0))
        .child(star_icon(12.0))
        .child(bold(g.points(p.id).to_string(), 11.0, palette::INK))
        .child(view().w(px(3.0)))
        .child(cards_icon(12.0))
        .child(bold(p.resources.total().to_string(), 11.0, palette::MUTED))
        .child((cards > 0).then(|| {
            view()
                .flex_row()
                .items_center()
                .child(view().w(px(3.0)))
                .child(glyph_icon(Glyph::Knight, color, 12.0, 12.0))
                .child(bold(cards.to_string(), 11.0, palette::MUTED))
        }))
        .child({
            let wait = p.mana_ready_round.saturating_sub(g.round);
            view()
                .flex_row()
                .items_center()
                .child(view().w(px(3.0)))
                .child(mana_icon(12.0, wait == 0))
                .child((wait > 0).then(|| bold(wait.to_string(), 11.0, palette::MUTED)))
        });
    let chip = view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .pl(px(4.0))
        .pr(px(10.0))
        .py(px(4.0))
        .rounded(px(18.0))
        .bg(hexa(palette::PAPER, if active { 1.0 } else { 0.8 }))
        .border_2()
        .border_color(hex(if active { palette::GOLD } else { palette::PAPER_DIM }))
        .child(avatar(p.id, &p.name, 28.0, active))
        .child(
            view()
                .gap(px(1.0))
                .child(
                    bold(if me { format!("{} (you)", p.name) } else { p.name.clone() }, 11.5, palette::INK)
                        .number_of_lines(1)
                        .max_w(px(78.0)),
                )
                .child(stats),
        );
    if active { chip.shadow_lg() } else { chip }
}

/// The active player's chip; tapping it drops everyone's below it.
fn players_bar(app: AppState) -> impl IntoElement {
    let everyone = move || {
        let mv = app.view.get()?;
        let g = mv.game.as_ref()?;
        let finished = matches!(g.phase, Phase::Finished { .. });
        let chips: Vec<View> =
            g.players.iter().map(|p| player_chip(g, p, p.id == mv.seat, p.id == g.current && !finished)).collect();
        Some(view().gap(px(5.0)).items_start().child(chips))
    };
    let current = move || app.view.with(|v| v.as_ref().and_then(|v| v.game.as_ref()).map(|g| g.current));
    view()
        .flex_1()
        .items_start()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .pointer_events(PointerEvents::BoxNone)
                .child(dynamic(move || {
                    let mv = app.view.get()?;
                    let g = mv.game.as_ref()?;
                    let finished = matches!(g.phase, Phase::Finished { .. });
                    let current = &g.players[g.current.index()];
                    Some(
                        player_chip(g, current, current.id == mv.seat, !finished)
                            .active(|s| s.opacity(0.8))
                            .on_press(move || app.players_open.update(|o| *o = !*o)),
                    )
                }))
                .child(reaction_bubbles(app, current)),
        )
        .child(turn_queue(app))
        // Out of the column's flow, so opening it moves nothing else; made
        // once per opening, so it slides in only then.
        .child(when(app.players_open, move || {
            panel()
                .absolute()
                .top(px(46.0))
                .left(px(0.0))
                .w(px(240.0))
                .p(px(8.0))
                .gap(px(5.0))
                .items_start()
                .on_press(move || app.players_open.set(false))
                .bind_motion(&motion().entering(MotionPreset::FadeInDown))
                .child(dynamic(everyone))
        }))
}

/// What the board asks of the player now, if anything.
fn prompt(app: AppState, g: &Game, seat: PlayerId, names: &[String]) -> Option<String> {
    Some(match g.phase {
        Phase::Finished { winner } => format!("{} wins!", name(names, winner)),
        Phase::Setup { step, placing } if g.current == seat => match placing {
            SetupPlacing::Building => {
                format!("Tap a glowing spot for your starting {}", g.setup_building_kind(step).name())
            }
            SetupPlacing::Road { .. } => "Tap a glowing edge for a road beside it".to_owned(),
        },
        Phase::Setup { .. } => return None,
        Phase::Playing if g.current != seat => return None,
        Phase::Playing if g.dice.is_none() => "Your turn! Roll the dice".to_owned(),
        Phase::Playing => match app.mode.get() {
            Mode::Road => "Tap a glowing edge to build a road".to_owned(),
            Mode::Outpost => "Tap a spot on your roads for an outpost".to_owned(),
            Mode::UpgradeTown => "Tap an outpost to make it a town".to_owned(),
            Mode::UpgradeCity => "Tap a town to make it a castle".to_owned(),
            Mode::Deploy(k) => format!("Tap a tile beside your town or castle for the {}", k.name()),
            Mode::Cast(spell, first) => rules::spell_hint(spell, first.is_some()),
            Mode::Unit(u) => match g.unit(u).and_then(|u| u.blockade) {
                Some(Blockade::Holding) => "Your knight holds a blockade: it can't move or attack".to_owned(),
                Some(Blockade::Lifting { free_after }) => {
                    format!("Your knight is packing up: free after round {free_after}")
                }
                None => "Gold: move · Red: attack bandits and knights".to_owned(),
            },
            Mode::Robber(_) => "Tap any free tile to move the bandit".to_owned(),
            Mode::Idle if g.robber_relocation && g.robbers.is_empty() => {
                "You rolled 7: tap a tile to place a bandit".to_owned()
            }
            Mode::Idle if g.robber_relocation => "You rolled 7: tap a bandit to move it".to_owned(),
            Mode::Idle => return None,
        },
    })
}

fn inspect_text(g: &Game, names: &[String], i: Inspect) -> String {
    match i {
        Inspect::Tile(t) => {
            let tile = g.board.tile(t);
            let mut s = match g.structure_at(t) {
                Some(kind) => rules::structure_title(kind).to_owned(),
                None => format!("{} · {}", tile.resource.name(), tile.number),
            };
            if let Some(u) = g.unit_at(t) {
                let relics = match u.relics {
                    0 => String::new(),
                    1 => " carrying a relic".to_owned(),
                    n => format!(" carrying {n} relics"),
                };
                let blockade = match u.blockade {
                    None => String::new(),
                    Some(Blockade::Holding) => " blockading".to_owned(),
                    Some(Blockade::Lifting { free_after }) => format!(" packing up (free after round {free_after})"),
                };
                s += &format!(
                    " · {}'s {}{relics}{blockade}{}",
                    name(names, u.owner),
                    u.kind.name(),
                    if u.ready { "" } else { " (resting)" }
                );
            }
            if g.robber_at(t).is_some() {
                s += " · bandit (blocked)";
            }
            if let Some(e) = g.enchantment_at(t) {
                let left = match e.last_round {
                    Some(last) => format!("ends after round {last}"),
                    None => rules::charges_word(e.charges),
                };
                s += &format!(" · {} ({left})", e.spell.name());
            }
            if let Some(ward) = g.wards_over(t).find(|e| e.tile != t) {
                s += &format!(" · guarded by {}'s ward", name(names, ward.caster));
            }
            if g.luck == Some(t) {
                s += " · luck (next roll)";
            }
            if let Some(p) = g.players.iter().find(|p| g.ringed_tiles(p.id).contains(&t)) {
                s += &format!(" · ringed by {}", name(names, p.id));
                if !g.city_on(t) {
                    s += " (pays once a castle stands on it)";
                }
            }
            if let Some(item) = g.item_at(t) {
                let what = item.kind.map_or("an unknown item", rules::item_title);
                s += &format!(" · {what} (gone after round {})", item.last_round);
            }
            if let Some(kind) = g.structure_at(t) {
                let gain = rules::structure_gain(kind);
                s += &match g.structure_holder(t) {
                    Some(p) => format!(" · {} held by {} ({gain})", kind.name(), name(names, p)),
                    None if g.robber_at(t).is_some() => format!(" · {} held by a bandit", kind.name()),
                    None => format!(" · empty {}: a knight here gets {gain}", kind.name()),
                };
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
        .pointer_events(PointerEvents::BoxNone)
        .on_layout(move |f: &Bounds<Points>| app.hud.set(Hud { top: f.size.height.0, ..app.hud.get_untracked() }))
        .child(view().flex_row().items_start().gap(px(6.0)).child(players_bar(app)).child(
            view().items_end().gap(px(4.0)).child(dice(app)).child(dynamic(move || {
                let (round, goal) =
                    app.view.with(|v| v.as_ref().and_then(|v| v.game.as_ref().map(|g| (g.round, g.victory_points))))?;
                let online = app.connected.get();
                Some(bold(
                    if !online {
                        "reconnecting…".to_owned()
                    } else if round == 0 {
                        format!("SETUP · TO {goal}")
                    } else {
                        format!("ROUND {round} · TO {goal}")
                    },
                    10.0,
                    if online { 0xe7dfcb } else { palette::GOLD },
                ))
            })),
        ))
}

/// The strip over the dock: what the board asks (or what was tapped), the
/// latest action and the player's points. Tapping it opens the history
/// above it.
fn action_bar(app: AppState) -> impl IntoElement {
    view()
        .pointer_events(PointerEvents::BoxNone)
        .child(when(app.history_open, move || {
            panel()
                .mx(px(8.0))
                .mb(px(6.0))
                .px(px(12.0))
                .py(px(8.0))
                .gap(px(3.0))
                .on_press(move || app.history_open.set(false))
                .bind_motion(&motion().entering(MotionPreset::FadeInUp))
                .child(bold("History", 12.0, palette::MUTED))
                .child(dynamic(move || {
                    let mv = app.view.get()?;
                    let g = mv.game.as_ref()?;
                    let lines: Vec<Text> = g
                        .events
                        .iter()
                        .rev()
                        .filter_map(|e| describe(&mv.players, &e.event))
                        .take(14)
                        .map(|l| label(l, 12.0, palette::INK).number_of_lines(1))
                        .collect();
                    Some(view().gap(px(3.0)).child(lines))
                }))
        }))
        .child(dynamic(move || {
            let mv = app.view.get()?;
            let g = mv.game.as_ref()?;
            let seat = mv.seat;
            let ask = match app.inspect.get() {
                Some(i) => Some(inspect_text(g, &mv.players, i)),
                None => prompt(app, g, seat, &mv.players),
            };
            // The dice are still in the air: keep their result for the landing.
            let last = if app.dice_hidden.get() {
                Some("Rolling…".to_owned())
            } else {
                g.events.iter().rev().find_map(|e| describe(&mv.players, &e.event))
            };
            let (last_size, last_color) = if ask.is_some() { (11.0, 0xbdb7aa) } else { (13.0, 0xffffff) };
            Some(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .py(px(8.0))
                    .bg(hexa(0x000000, 0.6))
                    .rounded_tl(px(22.0))
                    .rounded_tr(px(22.0))
                    .active(|s| s.opacity(0.85))
                    .on_press(move || app.history_open.update(|o| *o = !*o))
                    .on_layout(move |f: &Bounds<Points>| {
                        app.hud.set(Hud { strip: f.size.height.0, ..app.hud.get_untracked() })
                    })
                    .child(
                        view()
                            .flex_1()
                            .gap(px(2.0))
                            .child(ask.map(|a| bold(a, 13.5, if g.is_active(seat) { 0xffe7a3 } else { 0xffffff })))
                            .child(last.map(|l| label(l, last_size, last_color).number_of_lines(1)))
                            .child(unit_command(app, g, seat)),
                    )
                    .child(view().flex_row().items_center().gap(px(4.0)).child(star_icon(16.0)).child(bold(
                        g.points(seat).to_string(),
                        16.0,
                        0xffffff,
                    ))),
            )
        }))
}

/// The selected knight's blockade order, under the prompt.
fn unit_command(app: AppState, g: &Game, seat: PlayerId) -> Option<View> {
    let Mode::Unit(id) = app.mode.get() else { return None };
    let (text, action, ok) = match g.unit(id)?.blockade {
        None => ("Blockade this tile", Action::LayBlockade { unit: id }, g.can_lay_blockade(seat, id).is_ok()),
        Some(Blockade::Holding) => {
            ("Lift the blockade", Action::LiftBlockade { unit: id }, g.can_lift_blockade(seat, id).is_ok())
        }
        Some(Blockade::Lifting { .. }) => return None,
    };
    Some(pill(text, palette::GOLD, palette::INK, ok && !app.busy.get(), move || {
        app.mode.set(Mode::Idle);
        app.send(action.clone());
    }))
}

// ----- Bottom -----

/// The player's resources and unit cards; resources just collected bounce
/// as their badges rise from the board.
fn inventory(app: AppState, g: &Game, seat: PlayerId, before: Option<Resources>) -> View {
    let me = &g.players[seat.index()];
    let can_deploy = g.is_active(seat) && g.dice.is_some();
    let delay = (app.gains_at.get() - now_ms()).max(0.0) as f32;
    let resources: Vec<View> = Resource::ALL
        .iter()
        .map(|&r| {
            let n = me.resources.get(r);
            let chip = view()
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .pl(px(4.0))
                .pr(px(8.0))
                .py(px(4.0))
                .rounded(px(12.0))
                .bg(hex(0xfffdf6))
                .border_1()
                .border_color(hexa(palette::INK, 0.12))
                .opacity(if n > 0 { 1.0 } else { 0.6 })
                .child(resource_icon(r, 26.0))
                .child(bold(n.to_string(), 18.0, palette::INK).min_w(px(11.0)));
            if before.is_some_and(|b| n > b.get(r)) {
                chip.bind_motion(&motion().entering(MotionPreset::Tada.spec().timing(|t| t.delay(delay))))
            } else {
                chip
            }
        })
        .collect();
    let card = |count: u32| {
        let mode = Mode::Deploy(UnitKind::Knight);
        let selected = app.mode.get_untracked() == mode;
        let enabled = can_deploy && count > 0;
        let v = view()
            .flex_row()
            .items_center()
            .gap(px(3.0))
            .px(px(6.0))
            .py(px(4.0))
            .rounded(px(12.0))
            .bg(hex(if selected { palette::GOLD } else { 0x4a3c30 }))
            .opacity(if count > 0 { 1.0 } else { 0.45 })
            .child(glyph_icon(Glyph::Knight, palette::player(seat), 22.0, 26.0))
            .child(bold(format!("×{count}"), 14.0, if selected { palette::INK } else { 0xffffff }));
        if enabled {
            v.active(|s| s.opacity(0.7).scale(0.96)).on_press(move || {
                app.inspect.set(None);
                app.mode.set(if app.mode.get_untracked() == mode { Mode::Idle } else { mode });
            })
        } else {
            v
        }
    };
    // Trading with the bank or the table, right beside what is traded.
    let trading = app.trade_open.get_untracked();
    let trade_enabled = g.is_active(seat) && g.dice.is_some() && matches!(g.phase, Phase::Playing) && g.trade.is_none();
    let trade = view()
        .items_center()
        .justify_center()
        .px(px(6.0))
        .py(px(4.0))
        .rounded(px(12.0))
        .bg(hex(if trading { palette::GOLD } else { 0xfffdf6 }))
        .border_1()
        .border_color(hex(if trading { 0xb8860b } else { palette::PAPER_DIM }))
        .opacity(if trade_enabled || trading { 1.0 } else { 0.45 })
        .child(glyph_icon(Glyph::Trade, palette::player(seat), 30.0, 26.0));
    let trade = if trade_enabled {
        trade.active(|s| s.opacity(0.7).scale(0.96)).on_press(move || app.trade_open.update(|o| *o = !*o))
    } else {
        trade
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .child(view().flex_row().flex_1().gap(px(4.0)).child(resources))
        .child(card(me.cards.knights))
        .child(trade)
}

/// What buying an item does: pick a place on the board, or buy a card.
enum Use {
    Place(Mode),
    Buy(UnitKind),
}

fn item_use(item: Item) -> Use {
    match item {
        Item::Road => Use::Place(Mode::Road),
        Item::Outpost => Use::Place(Mode::Outpost),
        Item::Town => Use::Place(Mode::UpgradeTown),
        Item::City => Use::Place(Mode::UpgradeCity),
        Item::Knight => Use::Buy(UnitKind::Knight),
    }
}

/// Why the player cannot use `item` right now, or `None` when they can.
fn item_blocker(g: &Game, seat: PlayerId, item: Item) -> Option<String> {
    if !g.is_active(seat) || !matches!(g.phase, Phase::Playing) {
        return Some("Wait for your turn".to_owned());
    }
    if g.dice.is_none() {
        return Some("Roll the dice first".to_owned());
    }
    if !g.players[seat.index()].resources.covers(&item.cost()) {
        return Some("Not enough resources".to_owned());
    }
    let usable = match item_use(item) {
        Use::Place(mode) => !board::options(g, seat, mode).is_empty(),
        Use::Buy(kind) => g.can_buy_card(seat, kind).is_ok(),
    };
    (!usable).then(|| item.no_place().to_owned())
}

/// Every build, upgrade and card with its price. A tap opens the item's card,
/// which says what it does and buys it; usable ones light up on the
/// player's turn.
fn tray(app: AppState, g: &Game, seat: PlayerId) -> View {
    let my_move = g.is_active(seat) && g.dice.is_some() && matches!(g.phase, Phase::Playing);
    let have = g.players[seat.index()].resources;
    let color = palette::player(seat);
    let current = app.mode.get_untracked();
    let tile = |glyph: Glyph, title: &str, selected: bool, opacity: f32| {
        view()
            .items_center()
            .gap(px(2.0))
            .px(px(6.0))
            .pt(px(5.0))
            .pb(px(6.0))
            .rounded(px(14.0))
            .min_w(px(68.0))
            .bg(hex(if selected { palette::GOLD } else { 0xfffdf6 }))
            .border_2()
            .border_color(hex(if selected { 0xb8860b } else { palette::PAPER_DIM }))
            .opacity(opacity)
            .child(glyph_icon(glyph, color, 40.0, 26.0))
            .child(bold(title.to_owned(), 11.5, palette::INK))
    };
    let buttons: Vec<View> = Item::ALL
        .into_iter()
        .map(|item| {
            let usable = item_blocker(g, seat, item).is_none();
            let selected = matches!(item_use(item), Use::Place(m) if m == current);
            let opacity = if usable || selected {
                1.0
            } else if my_move {
                0.62
            } else {
                0.5
            };
            tile(item.glyph(), item.title(), selected, opacity)
                .child(cost_row(&item.cost(), Some(&have), 14.0, 11.5))
                .active(|s| s.opacity(0.75).scale(0.95))
                .on_press(move || {
                    app.inspect.set(None);
                    if selected {
                        app.mode.set(Mode::Idle);
                    } else {
                        app.item_card.set(Some(item));
                    }
                })
        })
        .collect();
    // One card for all the spells: it opens the spellbook.
    let casting = matches!(current, Mode::Cast(..));
    let mana = mana_state(g, seat);
    let castable = my_move && (mana == "ready" || !g.players[seat.index()].scrolls.is_empty());
    let spells = tile(Glyph::Spellbook, "Spells", casting, if castable { 1.0 } else { 0.62 })
        .child(label(mana, 11.0, palette::MUTED))
        .active(|s| s.opacity(0.75).scale(0.95))
        .on_press(move || {
            app.inspect.set(None);
            if casting {
                app.mode.set(Mode::Idle);
            } else {
                app.spellbook_open.set(true);
            }
        });
    view().child(scroll().horizontal().child(view().flex_row().gap(px(6.0)).child(buttons).child(spells)))
}

/// The player's mana, in a word or two.
fn mana_state(g: &Game, seat: PlayerId) -> String {
    match g.players[seat.index()].mana_ready_round.saturating_sub(g.round) {
        0 => "ready".to_owned(),
        1 => "1 round".to_owned(),
        n => format!("{n} rounds"),
    }
}

/// Why the player cannot cast `spell` right now, or `None` when they can.
fn spell_blocker(g: &Game, seat: PlayerId, spell: Spell) -> Option<String> {
    if !g.is_active(seat) || !matches!(g.phase, Phase::Playing) {
        return Some("Wait for your turn".to_owned());
    }
    if g.dice.is_none() {
        return Some("Roll the dice first".to_owned());
    }
    // A scroll of the spell casts it free, whatever the mana.
    if !g.players[seat.index()].scrolls.contains(&spell) {
        let wait = g.players[seat.index()].mana_ready_round.saturating_sub(g.round);
        if wait > 0 {
            return Some(format!("Your mana refills in {wait} round{}", if wait == 1 { "" } else { "s" }));
        }
        if !g.players[seat.index()].resources.covers(&g.spell_price(seat, spell)) {
            return Some("Not enough resources".to_owned());
        }
    }
    if spell.targets() == 0 {
        return g.can_cast(seat, spell, &[]).err().map(|e| e.to_string());
    }
    board::options(g, seat, Mode::Cast(spell, None)).is_empty().then(|| "Nowhere to cast it right now".to_owned())
}

/// The spellbook: every spell with its price and what it does; pick one to
/// read it in full and cast it.
fn spellbook(app: AppState) -> impl IntoElement {
    let chosen = signal(Spell::Frost);
    let close = move || app.spellbook_open.set(false);
    // Made once per opening, so it rises into view only then; the mana line
    // and the spells update in place, so picking one does not flash.
    when(app.spellbook_open, move || {
        let status = dynamic(move || {
            let (mana, castle) = app.view.with(|v| {
                let v = v.as_ref()?;
                let g = v.game.as_ref()?;
                Some((mana_state(g, v.seat), g.has_city(v.seat)))
            })?;
            Some(view().child(label(format!("Mana: {mana}"), 12.0, palette::MUTED)).child(label(
                if castle { "Free: you have a castle" } else { "Free once you have a castle" },
                12.0,
                palette::MUTED,
            )))
        });
        let rows = dynamic(move || {
            let view_ = app.view.get()?;
            let g = view_.game.as_ref()?;
            let seat = view_.seat;
            let have = g.players[seat.index()].resources;
            let castle = g.has_city(seat);
            let picked = chosen.get();
            let busy = app.busy.get();
            let rows: Vec<View> = Spell::ALL
                .into_iter()
                .map(|spell| {
                    let open = spell == picked;
                    let free = g.players[seat.index()].scrolls.iter().filter(|&&s| s == spell).count();
                    let head = view()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(glyph_icon(Glyph::Spell(spell), palette::player(seat), 40.0, 40.0))
                        .child(
                            view()
                                .flex_1()
                                .gap(px(1.0))
                                .child(bold(rules::spell_title(spell), 15.0, palette::INK))
                                .child(label(rules::spell_summary(spell), 12.0, palette::MUTED))
                                .child((free > 0).then(|| {
                                    bold(
                                        format!("{free} free scroll{}", if free == 1 { "" } else { "s" }),
                                        12.0,
                                        palette::GO,
                                    )
                                })),
                        )
                        .child(if castle {
                            view().child(bold("Free", 13.0, palette::GO))
                        } else {
                            cost_row(&spell.cost(), Some(&have), 16.0, 12.0)
                        });
                    let row = view()
                        .px(px(10.0))
                        .py(px(8.0))
                        .gap(px(8.0))
                        .rounded(px(14.0))
                        .bg(hex(if open { 0xfff4d6 } else { 0xfffdf6 }))
                        .border_2()
                        .border_color(hex(if open { palette::GOLD } else { palette::PAPER_DIM }))
                        .on_press(move || chosen.set(spell))
                        .child(head);
                    if !open {
                        return row;
                    }
                    let blocker = spell_blocker(g, seat, spell);
                    let lines: Vec<Text> = rules::spell_rules(spell)
                        .into_iter()
                        .chain([rules::mana_rule()])
                        .map(|l| label(l, 13.0, palette::INK))
                        .collect();
                    let cast = pill(
                        format!("Cast {}", spell.name()),
                        palette::GO,
                        0xffffff,
                        blocker.is_none() && !busy,
                        move || {
                            app.spellbook_open.set(false);
                            if spell.targets() == 0 {
                                app.send(Action::CastSpell { spell, tiles: Vec::new() });
                            } else {
                                app.mode.set(Mode::Cast(spell, None));
                            }
                        },
                    );
                    row.child(view().gap(px(5.0)).child(lines))
                        .child(blocker.map(|why| bold(why, 13.0, palette::DANGER).self_center()))
                        .child(cast)
                })
                .collect();
            Some(view().px(px(12.0)).pb(px(16.0)).gap(px(8.0)).child(rows))
        });
        view()
            .absolute()
            .inset_0()
            .bg(hexa(0x000000, 0.55))
            .pt_safe_area_plus(px(16.0))
            .pb_safe_area_plus(px(16.0))
            .px(px(12.0))
            .on_press(close)
            .child(
                panel()
                    .flex_1()
                    .border_2()
                    .border_color(hex(palette::GOLD))
                    .on_press(|| {})
                    .bind_motion(&motion().entering(MotionPreset::FadeInUp))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(10.0))
                            .px(px(16.0))
                            .pt(px(14.0))
                            .pb(px(10.0))
                            .child(glyph_icon(Glyph::Spellbook, 0, 36.0, 40.0))
                            .child(view().flex_1().child(bold("Spells", 22.0, palette::INK)).child(status))
                            .child(pill("Close", palette::INK, 0xffffff, true, close)),
                    )
                    .child(scroll().flex_1().child(rows)),
            )
    })
}

/// An item's card: what it is, what it costs, what it does, and the button
/// that buys it (or why it cannot be bought now).
fn item_card(app: AppState) -> impl IntoElement {
    let open = computed(move || app.item_card.with(Option::is_some));
    let close = move || app.item_card.set(None);
    sheet(open, close, move || {
        let item = app.item_card.get()?;
        let view_ = app.view.get()?;
        let g = view_.game.as_ref()?;
        let seat = view_.seat;
        let have = g.players[seat.index()].resources;
        let blocker = item_blocker(g, seat, item);
        let lines: Vec<Text> = item.rules().into_iter().map(|l| label(l, 14.0, palette::INK)).collect();
        let buy = pill(item.verb(), palette::GO, 0xffffff, blocker.is_none() && !app.busy.get(), move || {
            app.item_card.set(None);
            match item_use(item) {
                Use::Place(mode) => app.mode.set(mode),
                Use::Buy(kind) => app.send(Action::BuyCard(kind)),
            }
        });
        Some(
            view()
                .gap(px(12.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(glyph_icon(item.glyph(), palette::player(seat), 72.0, 52.0))
                        .child(
                            view().flex_1().gap(px(2.0)).child(bold(item.title(), 22.0, palette::INK)).child(label(
                                item.summary(),
                                13.0,
                                palette::MUTED,
                            )),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(bold("Cost", 13.0, palette::MUTED))
                        .child(cost_row(&item.cost(), Some(&have), 22.0, 15.0)),
                )
                .child(view().gap(px(6.0)).child(lines))
                .child(blocker.map(|why| bold(why, 13.0, palette::DANGER).self_center()))
                .child(buy)
                .child(pill("Close", 0x8a8070, 0xffffff, true, close)),
        )
    })
}

/// An attack the player tapped, or a knight sent into a rival's ward: what
/// it will do, before it is done.
fn attack_confirm(app: AppState) -> impl IntoElement {
    let open = computed(move || app.confirm_attack.with(Option::is_some));
    let cancel = move || app.confirm_attack.set(None);
    sheet(open, cancel, move || {
        let action = app.confirm_attack.get()?;
        let view_ = app.view.get()?;
        let g = view_.game.as_ref()?;
        let (title, outcome, mover, victim, victim_color, verb) = match action {
            Action::Attack { unit, target } => {
                let (title, outcome) = rules::attack_outcome(g, &view_.players, unit, target)?;
                let (victim, color) = match target {
                    Target::Robber(_) => (Glyph::Robber, 0x2b2b2e),
                    Target::Unit(u) => (Glyph::Knight, palette::player(g.unit(u)?.owner)),
                };
                (title, outcome, g.unit(unit)?.owner, victim, color, "Attack")
            }
            Action::MoveUnit { to: tile, .. } | Action::Deploy { tile, .. } => {
                let (title, outcome, caster) = rules::ward_outcome(g, &view_.players, view_.seat, tile)?;
                let verb = if matches!(action, Action::Deploy { .. }) { "Deploy anyway" } else { "Move anyway" };
                (title, outcome, view_.seat, Glyph::Spell(Spell::Ward), palette::player(caster), verb)
            }
            _ => return None,
        };
        Some(
            view()
                .gap(px(12.0))
                .items_center()
                .child(bold(title, 22.0, palette::INK))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(glyph_icon(Glyph::Knight, palette::player(mover), 64.0, 52.0))
                        .child(bold("vs", 18.0, palette::DANGER))
                        .child(glyph_icon(victim, victim_color, 64.0, 52.0)),
                )
                .child(label(outcome, 15.0, palette::INK).text_center())
                .child(
                    pill(verb, palette::DANGER, 0xffffff, !app.busy.get(), move || {
                        app.confirm_attack.set(None);
                        app.mode.set(Mode::Idle);
                        app.send(action.clone());
                    })
                    .self_stretch(),
                )
                .child(pill("Cancel", 0x8a8070, 0xffffff, true, cancel).self_stretch()),
        )
    })
}

/// Every rule, piece and price, from the home screen or the game menu.
pub fn rulebook(app: AppState) -> impl IntoElement {
    // Made once per opening; the pages update in place, keeping the scroll.
    when(app.rules_open, move || {
        let pages = dynamic(move || {
            let color = app.seat().map_or(palette::PLAYERS[0], palette::player);
            let goal = app.view.with(|v| v.as_ref().map(|v| v.victory_points).filter(|&g| g > 0));
            let pages: Vec<View> = rules::rulebook(goal)
                .into_iter()
                .map(|s| {
                    let lines: Vec<Text> = s.lines.into_iter().map(|l| label(l, 13.5, palette::INK)).collect();
                    view()
                        .gap(px(6.0))
                        .pb(px(14.0))
                        .border_b(px(1.0))
                        .border_color(hexa(palette::INK, 0.1))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(10.0))
                                .child(s.glyph.map(|g| glyph_icon(g, color, 44.0, 32.0)))
                                .child(bold(s.title, 17.0, palette::INK).flex_1())
                                .child(s.cost.map(|c| cost_row(&c, None, 18.0, 13.0))),
                        )
                        .child(lines)
                })
                .collect();
            Some(view().px(px(16.0)).pb(px(16.0)).gap(px(14.0)).child(pages))
        });
        view()
            .absolute()
            .inset_0()
            .bg(hexa(0x000000, 0.55))
            .pt_safe_area_plus(px(16.0))
            .pb_safe_area_plus(px(16.0))
            .px(px(12.0))
            .items_center()
            .child(
                panel()
                    .flex_1()
                    .w_full()
                    .max_w(px(RULEBOOK_WIDTH))
                    .border_2()
                    .border_color(hex(palette::GOLD))
                    .bind_motion(&motion().entering(MotionPreset::FadeInUp))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .px(px(16.0))
                            .pt(px(14.0))
                            .pb(px(10.0))
                            .child(bold("How to play", 22.0, palette::INK).flex_1())
                            .child(pill("Close", palette::INK, 0xffffff, true, move || app.rules_open.set(false))),
                    )
                    .child(scroll().flex_1().child(pages)),
            )
    })
}

/// The one big button: roll, end the turn, or whose turn it is.
fn primary(app: AppState, g: &Game, seat: PlayerId, names: &[String]) -> Option<View> {
    let busy = app.busy.get();
    let current = name(names, g.current);
    Some(match g.phase {
        Phase::Playing if g.current == seat && g.dice.is_none() => {
            let breathe =
                MotionSpec::new(KeyframeTiming::new(750.0).infinite().direction(PlaybackDirection::Alternate))
                    .values(MotionChannel::Scale, [1.0, 1.05]);
            let roll = big_button(palette::GOLD, !busy, move || app.send(Action::Roll))
                .bg_linear(deg(180.0))
                .gradient_from(hex(0xffd978))
                .gradient_to(hex(0xe3a630))
                .border_2()
                .border_color(hex(0xffffff))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(die_face(5, 24.0))
                        .child(bold("Roll the dice", 19.0, palette::INK))
                        .child(die_face(3, 24.0)),
                );
            if busy { roll } else { roll.bind_motion(&motion().entering(breathe)) }
        }
        Phase::Playing if g.current == seat => big_button(palette::GO, !busy, move || {
            app.mode.set(Mode::Idle);
            app.trade_open.set(false);
            app.send(Action::EndTurn)
        })
        .child(bold("End turn", 18.0, 0xffffff)),
        Phase::Setup { .. } if g.current == seat => big_button(0xfff0c2, false, || {})
            .border_2()
            .border_color(hex(palette::GOLD))
            .child(bold("Your move: tap the board", 16.0, palette::INK)),
        Phase::Playing | Phase::Setup { .. } => big_button(0x3d4750, false, || {}).child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    view()
                        .size(px(14.0))
                        .rounded(px(7.0))
                        .bg(hex(palette::player(g.current)))
                        .border_1()
                        .border_color(hex(0xffffff)),
                )
                .child(bold(format!("{current} is playing…"), 16.0, 0xffffff)),
        ),
        Phase::Finished { .. } => return None,
    })
}

fn big_button(fill: u32, enabled: bool, on: impl Fn() + 'static) -> View {
    let v = view().h(px(52.0)).rounded(px(26.0)).items_center().justify_center().bg(hex(fill)).shadow_md();
    if enabled { v.active(|s| s.opacity(0.8).scale(0.97)).on_press(on) } else { v }
}

/// The open offer: answer it, or watch it as its maker. Only a new offer
/// makes the panel again (one to the player bounces in); what changes while
/// it stands, who declined and what the player can pay, updates in place.
fn trade_offer(app: AppState) -> impl IntoElement {
    let shown = computed(move || {
        app.view.with(|v| {
            let v = v.as_ref()?;
            let o = v.game.as_ref()?.trade.as_ref()?;
            let for_me = o.to.is_none_or(|p| p == v.seat) && !o.declined.contains(&v.seat);
            (o.from == v.seat || for_me).then_some((o.from == v.seat, o.from, o.give, o.want))
        })
    });
    // The player's resources, tracked by whoever reads them.
    let have = move || {
        app.view.with(|v| {
            v.as_ref().and_then(|v| v.game.as_ref().map(|g| g.players[v.seat.index()].resources)).unwrap_or_default()
        })
    };
    dynamic(move || {
        let (mine, from, give, want) = shown.get()?;
        if mine {
            let answers = dynamic(move || {
                let text = app.view.with(|v| {
                    let v = v.as_ref()?;
                    let o = v.game.as_ref()?.trade.as_ref()?;
                    Some(if o.declined.is_empty() {
                        "Waiting for answers…".to_owned()
                    } else {
                        let names: Vec<String> = o.declined.iter().map(|&p| name(&v.players, p)).collect();
                        format!("Declined by {}", names.join(", "))
                    })
                })?;
                Some(label(text, 11.0, palette::MUTED))
            });
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
                            .gap(px(4.0))
                            .child(bold("Your offer", 12.0, palette::MUTED))
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(cost_row(&give, None, 20.0, 14.0))
                                    .child(glyph_icon(Glyph::Trade, 0, 26.0, 20.0))
                                    .child(cost_row(&want, None, 20.0, 14.0)),
                            )
                            .child(answers),
                    )
                    .child(pill("Withdraw", 0x8a8070, 0xffffff, true, move || app.send(Action::CancelTrade))),
            );
        }
        let from = untracked(move || app.view.with(|v| v.as_ref().map(|v| name(&v.players, from)))).unwrap_or_default();
        let answer = dynamic(move || {
            let have = have();
            Some(
                view()
                    .gap(px(8.0))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .child(label("You get", 12.0, palette::MUTED))
                            .child(cost_row(&give, None, 22.0, 15.0))
                            .child(label("for", 12.0, palette::MUTED))
                            .child(cost_row(&want, Some(&have), 22.0, 15.0)),
                    )
                    .child(
                        view()
                            .flex_row()
                            .gap(px(8.0))
                            .child(pill("Accept", palette::GO, 0xffffff, have.covers(&want), move || {
                                app.send(Action::AcceptTrade)
                            }))
                            .child(pill("Decline", 0x8a8070, 0xffffff, true, move || app.send(Action::DeclineTrade))),
                    ),
            )
        });
        Some(
            panel()
                .px(px(12.0))
                .py(px(10.0))
                .gap(px(8.0))
                .border_2()
                .border_color(hex(palette::GOLD))
                .bind_motion(&motion().entering(MotionPreset::BounceIn))
                .child(bold(format!("{from} offers a trade"), 14.0, palette::INK))
                .child(answer),
        )
    })
}

/// Trading: four of a kind for one with the bank, or an offer to the other
/// players. Only opening and closing rebuild the panel; each part tracks
/// what it shows, so steppers and choices update without a flash.
fn trade_composer(app: AppState) -> impl IntoElement {
    let give = signal(Resources::ZERO);
    let want = signal(Resources::ZERO);
    let to = signal(None::<PlayerId>);
    let bank_give = signal(None::<Resource>);
    let bank_get = signal(None::<Resource>);
    let open = computed(move || {
        app.trade_open.get()
            && app.view.with(|v| {
                v.as_ref()
                    .and_then(|v| v.game.as_ref().map(|g| (g, v.seat)))
                    .is_some_and(|(g, seat)| g.is_active(seat) && g.dice.is_some() && g.trade.is_none())
            })
    });
    // The player's resources, tracked by whoever reads them.
    let mine = move || {
        app.view.with(|v| {
            v.as_ref().and_then(|v| v.game.as_ref().map(|g| g.players[v.seat.index()].resources)).unwrap_or_default()
        })
    };
    let stepper_row = move |title: &'static str, side: Signal<Resources>, limited: bool| {
        let cells: Vec<View> = Resource::ALL
            .iter()
            .map(|&r| {
                view().flex_1().items_center().gap(px(3.0)).child(resource_icon(r, 22.0)).child(dynamic(move || {
                    let n = side.get().get(r);
                    let max = if limited { mine().get(r) } else { 9 };
                    Some(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(3.0))
                            .child(step_button("−", n > 0, move || side.update(|s| *s.get_mut(r) -= 1)))
                            .child(bold(n.to_string(), 15.0, palette::INK).min_w(px(14.0)).text_center())
                            .child(step_button("+", n < max, move || side.update(|s| *s.get_mut(r) += 1))),
                    )
                }))
            })
            .collect();
        view().gap(px(4.0)).child(label(title, 11.0, palette::MUTED)).child(view().flex_row().child(cells))
    };
    let choice = |r: Resource, selected: bool, enabled: bool, count: u32| {
        view()
            .flex_1()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(3.0))
            .py(px(4.0))
            .rounded(px(10.0))
            .bg(hex(if selected { palette::GOLD } else { 0xfffdf6 }))
            .border_2()
            .border_color(hex(if selected { 0xb8860b } else { palette::PAPER_DIM }))
            .opacity(if enabled { 1.0 } else { 0.35 })
            .pointer_events(if enabled { PointerEvents::Auto } else { PointerEvents::None })
            .child(resource_icon(r, 24.0))
            .child(bold(format!("×{count}"), 13.0, palette::INK))
    };
    let bank = move || {
        dynamic(move || {
            let have = mine();
            let (picked_give, picked_get) = (bank_give.get(), bank_get.get());
            let rate = balance::BANK_TRADE_RATE;
            let gives: Vec<View> = Resource::ALL
                .iter()
                .map(|&r| {
                    choice(r, picked_give == Some(r), have.get(r) >= rate, rate).on_press(move || {
                        bank_give.set(Some(r));
                        if bank_get.get_untracked() == Some(r) {
                            bank_get.set(None);
                        }
                    })
                })
                .collect();
            let gets: Vec<View> = Resource::ALL
                .iter()
                .map(|&r| {
                    choice(r, picked_get == Some(r), picked_give != Some(r), 1).on_press(move || bank_get.set(Some(r)))
                })
                .collect();
            let ready = picked_give.zip(picked_get).is_some_and(|(g, w)| g != w && have.get(g) >= rate);
            Some(
                view()
                    .gap(px(6.0))
                    .child(bold(format!("Bank: {rate} of a kind for 1"), 12.0, palette::MUTED))
                    .child(view().flex_row().gap(px(4.0)).child(gives))
                    .child(view().flex_row().gap(px(4.0)).child(gets))
                    .child(pill(
                        format!("Trade {rate} for 1"),
                        palette::GO,
                        0xffffff,
                        ready && !app.busy.get(),
                        move || {
                            if let (Some(give), Some(get)) = (bank_give.get_untracked(), bank_get.get_untracked()) {
                                app.send(Action::TradeWithBank { give, get });
                            }
                            bank_give.set(None);
                            bank_get.set(None);
                        },
                    )),
            )
        })
    };
    let targets = move || {
        dynamic(move || {
            let players = app.view.with(|v| v.as_ref().map(|v| (v.players.clone(), v.seat)));
            let (names, seat) = players?;
            let chips: Vec<View> = std::iter::once(None)
                .chain((0..names.len() as u8).map(PlayerId).filter(|&p| p != seat).map(Some))
                .map(|target| {
                    let title = target.map_or("Anyone".to_owned(), |p| name(&names, p));
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
            Some(view().flex_row().flex_wrap().gap(px(5.0)).child(chips))
        })
    };
    let propose = move || {
        dynamic(move || {
            let ready = !give.get().is_empty() && !want.get().is_empty() && mine().covers(&give.get());
            Some(pill("Propose", palette::GO, 0xffffff, ready, move || {
                app.send(Action::ProposeTrade {
                    to: to.get_untracked(),
                    give: give.get_untracked(),
                    want: want.get_untracked(),
                });
                give.set(Resources::ZERO);
                want.set(Resources::ZERO);
                app.trade_open.set(false);
            }))
        })
    };
    dynamic(move || {
        if !open.get() {
            return None;
        }
        Some(
            panel()
                .px(px(12.0))
                .py(px(10.0))
                .gap(px(8.0))
                .bind_motion(&motion().entering(MotionPreset::FadeInUp))
                .child(bank())
                .child(view().h(px(1.0)).bg(hexa(palette::INK, 0.12)))
                .child(bold("Offer to players", 12.0, palette::MUTED))
                .child(stepper_row("You give", give, true))
                .child(stepper_row("You get", want, false))
                .child(targets())
                .child(view().flex_row().gap(px(8.0)).child(propose()).child(pill(
                    "Close",
                    0x8a8070,
                    0xffffff,
                    true,
                    move || app.trade_open.set(false),
                ))),
        )
    })
}

fn step_button(t: &'static str, enabled: bool, on: impl Fn() + 'static) -> View {
    let v = view()
        .size(px(26.0))
        .rounded(px(13.0))
        .items_center()
        .justify_center()
        .bg(hex(palette::PAPER_DIM))
        .opacity(if enabled { 1.0 } else { 0.35 })
        .child(bold(t, 15.0, palette::INK));
    if enabled { v.active(|s| s.opacity(0.6)).on_press(on) } else { v }
}

fn bottom(app: AppState) -> impl IntoElement {
    // The resources last shown, to tell which just grew.
    let shown = stored(None::<Resources>);
    view()
        .absolute()
        .left(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
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
                    .bind_motion(&motion().entering(MotionPreset::FadeInUp))
                    .child(bold(t, 13.0, 0xffffff))
            })
        }))
        .child(
            view()
                .px(px(8.0))
                .gap(px(6.0))
                .pointer_events(PointerEvents::BoxNone)
                .child(trade_composer(app))
                .child(trade_offer(app)),
        )
        .child(reaction_picker(app))
        .child(view().pointer_events(PointerEvents::BoxNone).child(action_bar(app)).child(dynamic(move || {
            let view_ = app.view.get()?;
            let g = view_.game.as_ref()?;
            let _ = app.mode.get();
            let _ = app.trade_open.get();
            let seat = view_.seat;
            let mine = g.players[seat.index()].resources;
            let before = shown.replace(Some(mine));
            Some(
                view()
                    .bg_linear(deg(180.0))
                    .gradient_from(hex(0xfbf6ea))
                    .gradient_to(hex(0xe9dfc6))
                    .border_t(px(2.0))
                    .border_color(hex(if g.is_active(seat) { palette::GOLD } else { 0xcbbf9f }))
                    .shadow_lg()
                    .px(px(8.0))
                    .pt(px(8.0))
                    .pb_safe_area_plus(px(8.0))
                    .gap(px(7.0))
                    .on_layout(move |f: &Bounds<Points>| {
                        app.hud.set(Hud { dock: f.size.height.0, ..app.hud.get_untracked() })
                    })
                    .child(inventory(app, g, seat, before))
                    .child(tray(app, g, seat))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(reaction_button(app))
                            .child(primary(app, g, seat, &view_.players).map(|p| p.flex_1()))
                            .child(menu_button(app)),
                    ),
            )
        })))
}

/// Over the action strip on the left while the board is zoomed in on a
/// tile: back to the whole island. Out of the bottom HUD's flow and under
/// it, so trading, reactions and the history neither move for it nor are
/// moved by it: they open over it.
fn recenter_button(app: AppState) -> impl IntoElement {
    // How far above the bottom edge it stands, while zoomed in: made again
    // only when that changes, so tapping tiles leaves it be.
    let above = computed(move || {
        app.focus.with(Option::is_some).then(|| {
            let hud = app.hud.get();
            hud.strip + hud.dock + 6.0
        })
    });
    dynamic(move || {
        let above = above.get()?;
        Some(
            view()
                .absolute()
                .left(px(8.0))
                .bottom(px(above))
                .child(pill("Recenter", palette::PAPER, palette::INK, true, move || app.look_at(None)).shadow_md()),
        )
    })
}

fn winner(app: AppState) -> impl IntoElement {
    let won = computed(move || app.view.with(|v| v.as_ref().and_then(|v| v.game.as_ref()?.winner()).is_some()));
    // Made once, so it bounces in once; the scores update in place.
    when(won, move || {
        let content = dynamic(move || {
            let view_ = app.view.get()?;
            let g = view_.game.as_ref()?;
            let w = g.winner()?;
            let me = w == view_.seat;
            let color = palette::player(w);
            let scores: Vec<View> = g
                .players
                .iter()
                .map(|p| {
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            view()
                                .size(px(12.0))
                                .rounded(px(6.0))
                                .bg(hex(palette::player(p.id)))
                                .border_1()
                                .border_color(hex(palette::INK)),
                        )
                        .child(bold(p.name.clone(), 13.0, palette::INK))
                        .child(star_icon(13.0))
                        .child(bold(g.points(p.id).to_string(), 13.0, palette::INK))
                })
                .collect();
            Some(
                view()
                    .items_center()
                    .gap(px(12.0))
                    .child(glyph_icon(Glyph::Building(BuildingKind::City), color, 80.0, 60.0))
                    .child(bold(
                        if me { "You win!".to_owned() } else { format!("{} wins!", name(&view_.players, w)) },
                        30.0,
                        palette::INK,
                    ))
                    .child(view().flex_row().flex_wrap().justify_center().gap(px(10.0)).child(scores))
                    .child(pill("Leave match", palette::INK, 0xffffff, true, move || app.leave())),
            )
        });
        view().absolute().inset_0().items_center().justify_center().bg(hexa(0x000000, 0.5)).child(
            panel()
                .px(px(30.0))
                .py(px(26.0))
                .border_2()
                .border_color(hex(palette::GOLD))
                .bind_motion(&motion().entering(MotionPreset::BounceIn))
                .child(content),
        )
    })
}

/// The round gear button beside the main button: it opens the menu.
fn menu_button(app: AppState) -> View {
    let gear = canvas(move || {
        let (s, c) = (28.0_f32, 14.0_f32);
        let (outer, inner) = (s * 0.48, s * 0.36);
        let mut p = Painter::new().begin_path();
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            let at = |r: f32, a: f32| (c + r * a.cos(), c + r * a.sin());
            let pts = [at(inner, a - 0.36), at(outer, a - 0.2), at(outer, a + 0.2), at(inner, a + 0.36)];
            for (i, (x, y)) in pts.into_iter().enumerate() {
                p = if k == 0 && i == 0 { p.move_to(x, y) } else { p.line_to(x, y) };
            }
        }
        p.close_path()
            .fill_color(hex(palette::INK))
            .fill()
            .begin_path()
            .arc(c, c, s * 0.15, 0.0, std::f32::consts::TAU)
            .fill_color(hex(0xfffdf6))
            .fill()
    })
    .size(px(28.0));
    round_button(move || app.menu.set(Menu::Open)).child(gear)
}

/// A round button the main button's height, on either side of it.
fn round_button(on: impl Fn() + 'static) -> View {
    view()
        .size(px(52.0))
        .rounded(px(26.0))
        .items_center()
        .justify_center()
        .bg(hex(0xfffdf6))
        .border_2()
        .border_color(hex(palette::PAPER_DIM))
        .shadow_md()
        .active(|s| s.opacity(0.7).scale(0.94))
        .on_press(on)
}

/// The round button across from the menu: it opens the reactions.
fn reaction_button(app: AppState) -> View {
    round_button(move || app.reactions_open.update(|o| *o = !*o))
        .child(text(Reaction::Wave.emoji()).text_size(px(24.0)))
}

/// The reactions to pick from, over the dock.
fn reaction_picker(app: AppState) -> impl IntoElement {
    dynamic(move || {
        if !app.reactions_open.get() {
            return None;
        }
        let choices: Vec<View> = Reaction::ALL
            .into_iter()
            .map(|r| {
                view()
                    .size(px(44.0))
                    .rounded(px(22.0))
                    .items_center()
                    .justify_center()
                    .active(|s| s.opacity(0.6).scale(0.9))
                    .on_press(move || app.react(r))
                    .child(text(r.emoji()).text_size(px(26.0)))
            })
            .collect();
        Some(
            panel()
                .self_start()
                .ml(px(8.0))
                .flex_row()
                .p(px(4.0))
                .gap(px(2.0))
                .rounded(px(26.0))
                .bind_motion(&motion().entering(MotionPreset::FadeInUp))
                .child(choices),
        )
    })
}

/// The reactions `seat` (read reactively) just sent, each a chat bubble to
/// the right of their chip for a moment.
fn reaction_bubbles(app: AppState, seat: impl Fn() -> Option<PlayerId> + 'static) -> impl IntoElement {
    let theirs = computed(move || {
        let seat = seat();
        app.reactions_shown.with(|s| s.iter().filter(|r| Some(r.seat) == seat).copied().collect::<Vec<_>>())
    });
    // One bubble per reaction, kept while it shows, so each pops in only once.
    let bubbles = each(
        theirs,
        |r: &SentReaction| r.seq,
        move |r, _| {
            let r = r.get_untracked();
            // The emoji in it does a little dance and keeps bobbing while it
            // shows.
            let emoji = view().bind_motion(&motion().entering(MotionPreset::Float)).child(
                text(r.reaction.emoji()).text_size(px(26.0)).bind_motion(&motion().entering(MotionPreset::Tada)),
            );
            // The bubble's tail, pointing back at the chip.
            let tail = view()
                .absolute()
                .top(px(0.0))
                .bottom(px(0.0))
                .left(px(-5.0))
                .justify_center()
                .child(view().size(px(12.0)).bg(hex(palette::PAPER)).rotate(deg(45.0)));
            view()
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(18.0))
                .bg(hex(palette::PAPER))
                .shadow_lg()
                .bind_motion(&motion().entering(MotionPreset::PopIn))
                .child(tail)
                .child(emoji)
        },
    );
    view().flex_row().items_center().gap(px(6.0)).pointer_events(PointerEvents::None).child(bubbles)
}

/// A dimmed screen with a card in the middle while `open`. Made once per
/// opening, so the card zooms in only then; what it shows (`content`)
/// updates in place. The card swallows its own taps; `dismiss` hears the
/// taps outside it.
fn sheet(
    open: Memo<bool>,
    dismiss: impl Fn() + Copy + 'static,
    content: impl Fn() -> Option<View> + Copy + 'static,
) -> impl IntoElement {
    when(open, move || {
        view()
            .absolute()
            .inset_0()
            .items_center()
            .justify_center()
            .px(px(24.0))
            .bg(hexa(0x000000, 0.5))
            .on_press(dismiss)
            .child(
                panel()
                    .w_full()
                    .max_w(px(MENU_WIDTH))
                    .px(px(20.0))
                    .py(px(20.0))
                    .border_2()
                    .border_color(hex(palette::GOLD))
                    .on_press(|| {})
                    .bind_motion(&motion().entering(MotionPreset::ZoomIn))
                    .child(dynamic(content)),
            )
    })
}

/// Leaving the match, or (for the host) ending it for everyone, each asked
/// twice. Going from the menu to a question keeps the card.
fn menu(app: AppState) -> impl IntoElement {
    let open =
        computed(move || app.menu.get() != Menu::Closed && app.view.with(|v| v.as_ref().is_some_and(|v| !v.ended)));
    sheet(
        open,
        move || app.menu.set(Menu::Closed),
        move || {
            let choice = app.menu.get();
            let view_ = app.view.get()?;
            let host = view_.seat == PlayerId(0);
            let busy = app.busy.get();
            let back = move || app.menu.set(Menu::Open);
            let card = view().gap(px(12.0));
            Some(match choice {
            Menu::Open => card
                .child(bold(format!("Match {}", view_.code), 20.0, palette::INK).self_center())
                .child(pill("Resume", palette::GO, 0xffffff, true, move || app.menu.set(Menu::Closed)))
                .child(pill("How to play", palette::INK, 0xffffff, true, move || {
                    app.menu.set(Menu::Closed);
                    app.rules_open.set(true);
                }))
                .child(pill("Leave match", 0x8a8070, 0xffffff, true, move || app.menu.set(Menu::ConfirmLeave)))
                .child(host.then(|| {
                    pill("End match for everyone", palette::DANGER, 0xffffff, true, move || {
                        app.menu.set(Menu::ConfirmEnd)
                    })
                })),
            Menu::ConfirmLeave => card
                .child(bold("Leave this match?", 20.0, palette::INK))
                .child(label(
                    if host {
                        "Your seat stays and the others wait on your turns. To stop the game for everyone, end the match instead."
                    } else {
                        "Your seat stays and the others wait on your turns until the host ends the match."
                    },
                    14.0,
                    palette::MUTED,
                ))
                .child(pill("Leave", palette::DANGER, 0xffffff, true, move || app.leave()))
                .child(pill("Back", 0x8a8070, 0xffffff, true, back)),
            Menu::ConfirmEnd => card
                .child(bold("End the match for everyone?", 20.0, palette::INK))
                .child(label("Every player goes back to the home screen. This can't be undone.", 14.0, palette::MUTED))
                .child(pill("End match", palette::DANGER, 0xffffff, !busy, move || app.end_match()))
                .child(pill("Back", 0x8a8070, 0xffffff, true, back)),
            Menu::Closed => return None,
        })
        },
    )
}

/// The host ended the match: everyone heads home.
pub fn ended(app: AppState) -> impl IntoElement {
    let open = computed(move || app.view.with(|v| v.as_ref().is_some_and(|v| v.ended)));
    sheet(
        open,
        || {},
        move || {
            let view_ = app.view.get()?;
            let by_me = view_.seat == PlayerId(0);
            let host = name(&view_.players, PlayerId(0));
            Some(
                view()
                    .gap(px(12.0))
                    .items_center()
                    .child(bold("Match ended", 26.0, palette::INK))
                    .child(label(
                        if by_me { "You ended the match.".to_owned() } else { format!("{host} ended the match.") },
                        14.0,
                        palette::MUTED,
                    ))
                    .child(pill("Back to home", palette::GO, 0xffffff, true, move || app.leave()).self_stretch()),
            )
        },
    )
}

/// A gold frame that glows around the screen while it is the player's turn.
fn turn_glow(app: AppState) -> impl IntoElement {
    let mine = computed(move || {
        app.view.with(|v| v.as_ref().and_then(|v| v.game.as_ref().map(|g| g.is_active(v.seat)))).unwrap_or(false)
    });
    dynamic(move || {
        mine.get().then(|| {
            view()
                .absolute()
                .inset_0()
                .border(px(5.0))
                .border_color(hexa(palette::GOLD, 0.9))
                .pointer_events(PointerEvents::None)
                .bind_motion(&motion().entering(MotionPreset::Glow))
        })
    })
}

/// The game screen: the board, the turn frame, the HUD, the celebrations.
pub fn game_screen(app: AppState) -> impl IntoElement {
    // The clock rests in the background; coming back, it picks up again.
    effect(move || {
        if lifecycle() != nsh_core::platform::Lifecycle::Background {
            untracked(move || app.resume_clock());
        }
    });
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
        // An attack waiting for confirmation lapses when the turn does.
        if !g.is_active(view_.seat) {
            app.confirm_attack.set(None);
        }
    });
    view()
        .flex_1()
        .bg(hex(palette::TABLE))
        .child(board::board_view(app))
        .child(turn_glow(app))
        .child(top(app))
        .child(recenter_button(app))
        .child(bottom(app))
        .child(winner(app))
        .child(fx::fx_layer(app))
        .child(item_card(app))
        .child(spellbook(app))
        .child(attack_confirm(app))
        .child(menu(app))
        .child(ended(app))
}
