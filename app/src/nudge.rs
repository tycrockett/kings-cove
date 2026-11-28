//! Tips: a small bubble above the dock that points at what the player can
//! do right now (build something, deploy a knight, cast a spell, trade, or
//! end the turn). The rules for when one is on offer live here; when one
//! shows and hides is decided in `AppState`.

use kings_cove_engine::{Game, Phase, PlayerId, Resource, Spell, UnitKind, balance};
use nsh_core::prelude::*;

use crate::{
    board,
    hud::{bold, item_blocker, spell_blocker, undo_shown},
    palette::{self, hex, hexa},
    rules::Item,
    state::{AppState, Mode},
};

/// How far the dock's content sits in from the screen's edge.
pub const DOCK_INSET: f32 = 8.0;
/// The width of the line from the bubble down to its anchor.
const LINE_W: f32 = 2.0;
/// How far the ring stands out from its anchor.
const RING_GAP: f32 = 3.0;

/// Something worth pointing out. Listed in priority order.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Nudge {
    /// A knight token is held and a tile can take it.
    DeployKnight,
    /// The mana has refilled and a spell can be cast.
    MagicReady,
    /// A scroll casts this spell free.
    FreeSpell(Spell),
    /// The first castle was built: spells cost nothing now.
    FreeCastSpells,
    /// The round's hot resource sells at the market's best rate.
    MarketDeal(Resource),
    /// Enough of one resource to trade with the bank, and nothing to build.
    BankTrade(Resource),
    /// Nothing is left to do this turn.
    NothingLeft,
}

/// Where a tip points.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// The knight chip.
    Knight,
    /// The Spells chip.
    Spells,
    /// The trade button.
    Trade,
    /// The End turn button.
    EndTurn,
}

/// The rows of the dock a tip's bubble stands over.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Inventory,
    Bottom,
}

impl Anchor {
    fn row(self) -> Row {
        match self {
            Anchor::Knight | Anchor::Spells | Anchor::Trade => Row::Inventory,
            Anchor::EndTurn => Row::Bottom,
        }
    }
}

/// A handle on each element a tip can point at, so the tip's overlay can
/// follow it as it moves.
#[derive(Copy, Clone)]
pub struct Anchors {
    knight: ElementRef,
    spells: ElementRef,
    trade: ElementRef,
    end_turn: ElementRef,
}

impl Anchors {
    pub fn new() -> Self {
        Anchors { knight: element_ref(), spells: element_ref(), trade: element_ref(), end_turn: element_ref() }
    }

    pub fn get(&self, anchor: Anchor) -> ElementRef {
        match anchor {
            Anchor::Knight => self.knight,
            Anchor::Spells => self.spells,
            Anchor::Trade => self.trade,
            Anchor::EndTurn => self.end_turn,
        }
    }
}

/// A gold ring around `anchor` while the tip shows it: put it inside the
/// anchor, so it moves with it.
pub fn ring(app: AppState, anchor: Anchor) -> impl IntoElement {
    // Always there and only faded in, so it keeps its place as the anchor's
    // first child: under the anchor's icons and badges.
    let shown = move || app.nudge.get().filter(|_| !app.dice_hidden.get()).map(Nudge::anchor) == Some(anchor);
    view()
        .absolute()
        .top(px(-RING_GAP))
        .left(px(-RING_GAP))
        .right(px(-RING_GAP))
        .bottom(px(-RING_GAP))
        .rounded(px(16.0))
        .border_2()
        .border_color(hex(palette::GOLD))
        .opacity(move || if shown() { 1.0_f32 } else { 0.0 })
        .pointer_events(PointerEvents::None)
}

/// Where the anchors and the dock's rows were laid out, in the dock's own
/// points, so the tip can be drawn in one layer over the dock.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spots {
    /// Each anchor's frame inside its row: left from the dock's content,
    /// top from the row's.
    marks: Vec<(Anchor, Frame)>,
    /// The top of each row from the dock's top edge.
    rows: [f32; 2],
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
struct Frame {
    left: f32,
    top: f32,
    w: f32,
    h: f32,
}

impl Spots {
    /// Where `anchor` is: its left edge from the screen's, its top from the
    /// dock's, its width and height.
    fn frame(&self, anchor: Anchor) -> Option<Frame> {
        let f = self.marks.iter().find(|(a, _)| *a == anchor)?.1;
        Some(Frame { left: DOCK_INSET + f.left, top: self.rows[anchor.row() as usize] + f.top, ..f })
    }
}

/// Records where `anchor` is, from its frame inside its parent.
pub fn mark(app: AppState, anchor: Anchor, f: &Bounds<Points>) {
    let frame = Frame { left: f.origin.x.0, top: f.origin.y.0, w: f.size.width.0, h: f.size.height.0 };
    change(app, |s| match s.marks.iter_mut().find(|(a, _)| *a == anchor) {
        Some(m) => m.1 = frame,
        None => s.marks.push((anchor, frame)),
    });
}

/// Records where a dock row's top edge is.
pub fn mark_row(app: AppState, row: Row, f: &Bounds<Points>) {
    let y = f.origin.y.0;
    change(app, |s| {
        if (s.rows[row as usize] - y).abs() >= 0.5 {
            s.rows[row as usize] = y
        }
    });
}

/// Edits the spots, telling readers only when something moved.
fn change(app: AppState, edit: impl FnOnce(&mut Spots)) {
    let mut spots = app.spots.get_untracked();
    edit(&mut spots);
    if spots != app.spots.get_untracked() {
        app.spots.set(spots);
    }
}

impl Nudge {
    /// What the tip points at.
    pub fn anchor(self) -> Anchor {
        match self {
            Nudge::DeployKnight => Anchor::Knight,
            Nudge::MagicReady | Nudge::FreeSpell(_) | Nudge::FreeCastSpells => Anchor::Spells,
            Nudge::MarketDeal(_) | Nudge::BankTrade(_) => Anchor::Trade,
            Nudge::NothingLeft => Anchor::EndTurn,
        }
    }

    /// What the tip says.
    pub fn text(self, g: &Game) -> String {
        match self {
            Nudge::DeployKnight => "Deploy Knight".to_owned(),
            Nudge::MagicReady => "Magic is ready!".to_owned(),
            Nudge::FreeSpell(spell) => {
                let name = spell.name();
                let mut chars = name.chars();
                let head = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
                format!("Free spell: {head}{}", chars.as_str())
            }
            Nudge::FreeCastSpells => "Spells are free now!".to_owned(),
            Nudge::MarketDeal(r) => format!("Market: {} {} → 1 of anything", g.bank_rate(r), r.name()),
            Nudge::BankTrade(r) => format!("Trade {} {} with the bank", g.bank_rate(r), r.name()),
            Nudge::NothingLeft => "Nothing left: end your turn".to_owned(),
        }
    }

    /// Tips about something that just happened wait their turn when another
    /// tip is showing; the rest only show when nothing else is.
    pub fn queues(self) -> bool {
        matches!(self, Nudge::DeployKnight | Nudge::MagicReady | Nudge::FreeSpell(_) | Nudge::FreeCastSpells)
    }
}

/// Every tip on offer to `seat` now, highest priority first. Only on their
/// own turn, after the roll.
pub fn available(g: &Game, seat: PlayerId) -> Vec<Nudge> {
    let mut out = Vec::new();
    if !g.is_active(seat) || !matches!(g.phase, Phase::Playing) || g.dice.is_none() {
        return out;
    }
    let me = &g.players[seat.index()];
    let knight = me.cards.knights > 0 && !board::options(g, seat, Mode::Deploy(UnitKind::Knight)).is_empty();
    let castable: Vec<Spell> = Spell::ALL.into_iter().filter(|&s| spell_blocker(g, seat, s).is_none()).collect();
    if knight {
        out.push(Nudge::DeployKnight);
    }
    if me.mana_ready_round <= g.round && !castable.is_empty() {
        out.push(Nudge::MagicReady);
    }
    for &spell in &castable {
        if me.scrolls.contains(&spell) {
            out.push(Nudge::FreeSpell(spell));
        }
    }
    if g.has_city(seat) && !castable.is_empty() {
        out.push(Nudge::FreeCastSpells);
    }
    let can_bank =
        |give: Resource| Resource::ALL.iter().any(|&get| get != give && g.can_trade_with_bank(seat, give, get).is_ok());
    let nothing_to_build = Item::ALL.into_iter().all(|item| item_blocker(g, seat, item).is_some());
    let hot = g.market.map(|m| m.hot);
    if nothing_to_build {
        if let Some(r) = hot.filter(|&r| me.resources.get(r) >= g.bank_rate(r) && can_bank(r)) {
            out.push(Nudge::MarketDeal(r));
        }
        let plenty = Resource::ALL
            .into_iter()
            .filter(|&r| Some(r) != hot && me.resources.get(r) >= balance::BANK_TRADE_RATE && can_bank(r))
            .max_by_key(|&r| me.resources.get(r));
        if let Some(r) = plenty {
            out.push(Nudge::BankTrade(r));
        }
        if !knight && castable.is_empty() && !Resource::ALL.into_iter().any(can_bank) {
            out.push(Nudge::NothingLeft);
        }
    }
    out
}

/// The tips to show now that the game went from `before` to `after`,
/// highest priority first: those that just became available and were not
/// shown this turn (`seen`). A few are held back to the moment they
/// matter: magic on the player's first turn with the mana refilled, a free
/// spell when its scroll arrives, free casting when the first castle is built.
/// A new knight token (bought or found) brings Deploy Knight back, even when
/// tokens were already held or it already showed this turn.
pub fn fresh(before: &Game, after: &Game, seat: PlayerId, seen: &[Nudge]) -> Vec<Nudge> {
    let was = available(before, seat);
    let (b, a) = (&before.players[seat.index()], &after.players[seat.index()]);
    let new_token = a.cards.knights > b.cards.knights;
    available(after, seat)
        .into_iter()
        .filter(|n| (*n == Nudge::DeployKnight && new_token) || (!was.contains(n) && !seen.contains(n)))
        .filter(|n| match n {
            // Rounds count one turn each, so this is the first turn it is ready.
            Nudge::MagicReady => a.mana_ready_round > 0 && after.round == a.mana_ready_round,
            Nudge::FreeSpell(s) => {
                a.scrolls.iter().filter(|x| *x == s).count() > b.scrolls.iter().filter(|x| *x == s).count()
            }
            Nudge::FreeCastSpells => !before.has_city(seat),
            _ => true,
        })
        .collect()
}

/// The tip: a dark bubble above the action strip, so it covers none of the
/// dock, with a line down to what it is about (and [`ring`] around that).
/// An overlay anchored to that element, so it follows it as the tray
/// scrolls. It stays until the next press anywhere, which still does what
/// it presses.
pub fn layer(app: AppState) -> impl IntoElement {
    // The tip last drawn: it pops in once, not again as the tray scrolls.
    let drawn = stored(None::<Nudge>);
    dynamic(move || {
        let Some(nudge) = app.nudge.get().filter(|_| !app.dice_hidden.get()) else {
            drawn.replace(None);
            return None;
        };
        let text = app.game().map(|g| nudge.text(&g))?;
        let anchor = nudge.anchor();
        let f = app.spots.get().frame(anchor)?;
        let hud = app.hud.get();
        if hud.dock <= 0.0 {
            return None;
        }
        // The bubble sits over the strip, and over Undo and Recenter when
        // they show there; the line reaches down from it to the ring.
        let pills = undo_shown(app) || app.focus.with(Option::is_some);
        let line_h = hud.strip + f.top + if pills { 48.0 } else { 10.0 } - RING_GAP;
        // Near a side the bubble lines up with the anchor's edge, so it
        // stays on screen, and the line keeps to the anchor's middle.
        let x = f.left + f.w / 2.0;
        let width = window().width.0;
        let inset = px((f.w / 2.0 - LINE_W / 2.0).max(0.0));
        let line = view().w(px(LINE_W)).h(px(line_h.max(0.0))).bg(hex(palette::GOLD));
        let (align, col, line) = if x > width * 0.65 {
            (Align::End, view().items_end(), line.mr(inset))
        } else if x < width * 0.35 {
            (Align::Start, view().items_start(), line.ml(inset))
        } else {
            (Align::Center, view().items_center(), line)
        };
        let pop = drawn.replace(Some(nudge)) != Some(nudge);
        let body = view()
            .px(px(12.0))
            .py(px(7.0))
            .rounded(px(16.0))
            .bg(hexa(0x000000, 0.75))
            .child(bold(text, 13.0, 0xffffff));
        let body = if pop { body.bind_motion(&motion().entering(MotionPreset::PopIn)) } else { body };
        Some(
            view()
                .absolute()
                .size(px(0.0))
                .child(
                    overlay()
                        .anchor(&app.anchors.get(anchor))
                        .placement(Placement::Top)
                        .align(align)
                        .offset(px(RING_GAP))
                        .margin(px(4.0))
                        .child(col.pointer_events(PointerEvents::None).child(body).child(line)),
                )
                // An empty overlay hears every press, on the bubble too,
                // without taking it from what was pressed.
                .child(overlay().size(px(0.0)).on_press_outside(move |_: &Point<Points>| app.hide_nudge())),
        )
    })
}
