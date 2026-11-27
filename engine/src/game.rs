//! The game as a deterministic state machine: [`Game::apply`] validates an
//! [`Action`] from a player and executes it, drawing any randomness from the
//! [`Rng`] its caller (the server) owns.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    balance,
    board::Board,
    rng::Rng,
    types::{EdgeId, PlayerId, Resource, Resources, RobberId, TileId, UnitId, VertexId},
};

/// One building, upgraded in place: Outpost → Town → City.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BuildingKind {
    /// Every new building starts here: produces, worth nothing.
    Outpost,
    /// Produces, worth [`balance::TOWN_POINTS`], lets units deploy next to it.
    Town,
    /// Produces double, worth [`balance::CITY_POINTS`], lets units deploy next to it.
    City,
}

impl BuildingKind {
    pub fn points(self) -> u32 {
        match self {
            BuildingKind::Outpost => 0,
            BuildingKind::Town => balance::TOWN_POINTS,
            BuildingKind::City => balance::CITY_POINTS,
        }
    }

    /// Resources collected per matching adjacent tile.
    pub fn yield_per_tile(self) -> u32 {
        match self {
            BuildingKind::Outpost => balance::OUTPOST_YIELD,
            BuildingKind::Town => balance::TOWN_YIELD,
            BuildingKind::City => balance::CITY_YIELD,
        }
    }

    /// The level above, and what reaching it costs.
    pub fn upgrade(self) -> Option<(BuildingKind, Resources)> {
        match self {
            BuildingKind::Outpost => Some((BuildingKind::Town, balance::TOWN_UPGRADE_COST)),
            BuildingKind::Town => Some((BuildingKind::City, balance::CITY_UPGRADE_COST)),
            BuildingKind::City => None,
        }
    }

    /// Towns and cities: count toward robber capacity and deploy units.
    pub fn is_settlement(self) -> bool {
        self != BuildingKind::Outpost
    }

    pub fn name(self) -> &'static str {
        match self {
            BuildingKind::Outpost => "outpost",
            BuildingKind::Town => "town",
            BuildingKind::City => "castle",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Building {
    pub owner: PlayerId,
    pub kind: BuildingKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnitKind {
    /// Fights robbers and enemy knights.
    Knight,
}

impl UnitKind {
    pub const ALL: [UnitKind; 1] = [UnitKind::Knight];

    pub fn card_cost(self) -> Resources {
        match self {
            UnitKind::Knight => balance::KNIGHT_CARD_COST,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            UnitKind::Knight => "knight",
        }
    }
}

/// A spell a player casts on their turn. All share the caster's mana; each
/// has its own price, which a caster with a city does not pay.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Spell {
    /// A tile produces nothing for its next rolls.
    Frost,
    /// A tile produces double for its next rolls.
    Bounty,
    /// Moves a robber to any free tile.
    Lure,
    /// Two tiles trade numbers for their next rolls.
    Swap,
    /// The caster's next roll pays one of each resource their buildings touch.
    Windfall,
    /// The next roll, whoever makes it, comes up as a tile's number half the time.
    Luck,
}

impl Spell {
    pub const ALL: [Spell; 6] = [Spell::Frost, Spell::Bounty, Spell::Lure, Spell::Swap, Spell::Windfall, Spell::Luck];

    pub fn cost(self) -> Resources {
        match self {
            Spell::Frost => balance::FROST_COST,
            Spell::Bounty => balance::BOUNTY_COST,
            Spell::Lure => balance::LURE_COST,
            Spell::Swap => balance::SWAP_COST,
            Spell::Windfall => balance::WINDFALL_COST,
            Spell::Luck => balance::LUCK_COST,
        }
    }

    /// How many tiles it is cast on: for Lure the robber's, then where it
    /// goes; for Swap the two that trade numbers.
    pub fn targets(self) -> usize {
        match self {
            Spell::Frost | Spell::Bounty | Spell::Luck => 1,
            Spell::Lure | Spell::Swap => 2,
            Spell::Windfall => 0,
        }
    }

    /// Stops its tile producing.
    pub fn stops(self) -> bool {
        self == Spell::Frost
    }

    /// Doubles its tile's production.
    pub fn doubles(self) -> bool {
        self == Spell::Bounty
    }

    /// The rolls of its tile's number it lingers there for, for those that do.
    pub fn lasting(self) -> Option<u32> {
        match self {
            Spell::Frost | Spell::Bounty | Spell::Swap => Some(balance::SPELL_CHARGES),
            Spell::Lure | Spell::Windfall | Spell::Luck => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Spell::Frost => "frost",
            Spell::Bounty => "bounty",
            Spell::Lure => "lure",
            Spell::Swap => "swap",
            Spell::Windfall => "windfall",
            Spell::Luck => "luck",
        }
    }
}

/// A spell lingering on a tile (and, for Swap, its `partner`) for its
/// remaining `charges`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enchantment {
    pub tile: TileId,
    pub spell: Spell,
    pub caster: PlayerId,
    pub charges: u32,
    #[serde(default)]
    pub partner: Option<TileId>,
}

impl Enchantment {
    pub fn touches(&self, tile: TileId) -> bool {
        self.tile == tile || self.partner == Some(tile)
    }
}

/// Something that turns up on the island at the end of a round, for a
/// knight to walk onto.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemKind {
    /// [`balance::IRON_ORE_FIND`] for the finder.
    IronOre,
    /// [`balance::SUPPLIES_FIND`] of one resource, drawn when found.
    Supplies,
    /// The finder's mana refills at once.
    ManaCrystal,
    /// One free cast of a spell, drawn when found.
    Scroll,
    /// A knight card for the finder.
    Recruit,
    /// Carried by the knight, worth [`balance::RELIC_POINTS`] to its owner;
    /// dropped where the knight falls.
    Relic,
    /// The knight that steps on it falls.
    Trap,
    /// [`balance::HOARD_FIND`], but a robber appears next to it.
    Hoard,
}

impl ItemKind {
    pub const ALL: [ItemKind; 8] = [
        ItemKind::IronOre,
        ItemKind::Supplies,
        ItemKind::ManaCrystal,
        ItemKind::Scroll,
        ItemKind::Recruit,
        ItemKind::Relic,
        ItemKind::Trap,
        ItemKind::Hoard,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ItemKind::IronOre => "iron ore",
            ItemKind::Supplies => "supplies",
            ItemKind::ManaCrystal => "mana crystal",
            ItemKind::Scroll => "spell scroll",
            ItemKind::Recruit => "recruit",
            ItemKind::Relic => "relic",
            ItemKind::Trap => "trap",
            ItemKind::Hoard => "bandit's hoard",
        }
    }
}

/// An item lying on a tile until a knight walks onto it, a robber lands on
/// it, or it vanishes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub tile: TileId,
    /// What it is; `None` in a player's view (see [`Game::hide_items`])
    /// until a knight finds it, unless `revealed`.
    pub kind: Option<ItemKind>,
    /// Everyone knows what it is: relics a fallen knight dropped.
    pub revealed: bool,
    /// How many: more than one only for relics a knight dropped together.
    pub count: u32,
    /// It vanishes at the end of this round.
    pub last_round: u32,
}

/// A knight's blockade of the tile it stands on: the tile produces nothing,
/// and the knight neither moves nor attacks.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Blockade {
    Holding,
    /// Packing up: it holds until the end of round `free_after`.
    Lifting { free_after: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub id: UnitId,
    pub owner: PlayerId,
    pub kind: UnitKind,
    pub tile: TileId,
    /// May move or attack this turn.
    pub ready: bool,
    /// Relics it carries for its owner.
    #[serde(default)]
    pub relics: u32,
    #[serde(default)]
    pub blockade: Option<Blockade>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Robber {
    pub id: RobberId,
    pub tile: TileId,
    /// Skips the end-of-round moves up to and including this round
    /// (set when a player relocates it on a 7).
    pub frozen_through_round: u32,
}

/// Unit cards in a player's hand.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cards {
    pub knights: u32,
}

impl Cards {
    pub fn get(&self, kind: UnitKind) -> u32 {
        match kind {
            UnitKind::Knight => self.knights,
        }
    }

    fn get_mut(&mut self, kind: UnitKind) -> &mut u32 {
        match kind {
            UnitKind::Knight => &mut self.knights,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub resources: Resources,
    pub cards: Cards,
    /// The round from which the player may cast a spell again.
    #[serde(default)]
    pub mana_ready_round: u32,
    /// Windfall pays out on the player's next roll.
    #[serde(default)]
    pub windfall: bool,
    /// Free casts from found scrolls, one per entry.
    #[serde(default)]
    pub scrolls: Vec<Spell>,
}

/// The active player's open offer: they give `give` for `want`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeOffer {
    pub from: PlayerId,
    /// One player, or anyone when `None`.
    pub to: Option<PlayerId>,
    pub give: Resources,
    pub want: Resources,
    pub declined: Vec<PlayerId>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetupPlacing {
    /// The player places their building (a town in the first pass, an
    /// outpost in the second).
    Building,
    /// The player places a road touching the building just placed.
    Road { from: VertexId },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Snake-order initial placement: `step` indexes [`Game::setup_order`].
    Setup {
        step: usize,
        placing: SetupPlacing,
    },
    Playing,
    Finished {
        winner: PlayerId,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    Unit(UnitId),
    Robber(RobberId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    SetupBuilding(VertexId),
    SetupRoad(EdgeId),
    Roll,
    BuildRoad(EdgeId),
    BuildOutpost(VertexId),
    /// Outpost → town.
    UpgradeTown(VertexId),
    /// Town → city.
    UpgradeCity(VertexId),
    BuyCard(UnitKind),
    Deploy {
        kind: UnitKind,
        tile: TileId,
    },
    MoveUnit {
        unit: UnitId,
        to: TileId,
    },
    Attack {
        unit: UnitId,
        target: Target,
    },
    RelocateRobber {
        robber: RobberId,
        to: TileId,
    },
    /// After a 7 with no robber on the board: put a new one on a free tile.
    PlaceRobber {
        to: TileId,
    },
    ProposeTrade {
        to: Option<PlayerId>,
        give: Resources,
        want: Resources,
    },
    CancelTrade,
    AcceptTrade,
    DeclineTrade,
    /// `balance::BANK_TRADE_RATE` of one resource for one of another, with
    /// no other player involved.
    TradeWithBank {
        give: Resource,
        get: Resource,
    },
    /// `tiles` as the spell's `targets()` asks.
    CastSpell {
        spell: Spell,
        tiles: Vec<TileId>,
    },
    /// The knight blockades the tile it stands on.
    LayBlockade {
        unit: UnitId,
    },
    /// The knight starts packing up its blockade.
    LiftBlockade {
        unit: UnitId,
    },
    EndTurn,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleError {
    GameOver,
    NotYourTurn,
    NotSetup,
    SetupInProgress,
    MustRollFirst,
    AlreadyRolled,
    CannotAfford,
    VertexTaken,
    EdgeTaken,
    TileOccupied,
    NotConnected,
    NotUpgradable,
    NoCard,
    NotNextToYourTown,
    NoSuchUnit,
    NotYourUnit,
    UnitExhausted,
    NotAdjacent,
    InvalidTarget,
    NoRobberRelocation,
    RobberToMove,
    NoSuchRobber,
    NoTrade,
    TradeAlreadyOpen,
    InvalidTrade,
    NotTradeRecipient,
    TraderCannotPay,
    NoSuchPlayer,
    ManaEmpty,
    SpellTarget,
    WindfallPending,
    SettlementTooClose,
    Blockading,
    NotBlockading,
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RuleError::GameOver => "The game is over",
            RuleError::NotYourTurn => "It is not your turn",
            RuleError::NotSetup => "Initial placement is over",
            RuleError::SetupInProgress => "Finish initial placement first",
            RuleError::MustRollFirst => "Roll the dice first",
            RuleError::AlreadyRolled => "You already rolled this turn",
            RuleError::CannotAfford => "Not enough resources",
            RuleError::VertexTaken => "That spot already has a building",
            RuleError::EdgeTaken => "That edge already has a road",
            RuleError::TileOccupied => "That tile is occupied",
            RuleError::NotConnected => "Must connect to your road network",
            RuleError::NotUpgradable => "Only your own outposts and towns can be upgraded",
            RuleError::NoCard => "You have no such unit token",
            RuleError::NotNextToYourTown => "Units deploy next to your towns or castles",
            RuleError::NoSuchUnit => "No such unit",
            RuleError::NotYourUnit => "That unit is not yours",
            RuleError::UnitExhausted => "That unit cannot act until your next turn",
            RuleError::NotAdjacent => "Units only reach adjacent tiles",
            RuleError::InvalidTarget => "That unit cannot attack that",
            RuleError::NoRobberRelocation => "No bandit to relocate",
            RuleError::RobberToMove => "Move a bandit already on the board",
            RuleError::NoSuchRobber => "No such bandit",
            RuleError::NoTrade => "No open trade",
            RuleError::TradeAlreadyOpen => "A trade is already open",
            RuleError::InvalidTrade => "A trade must give and ask for something",
            RuleError::NotTradeRecipient => "That trade is not offered to you",
            RuleError::TraderCannotPay => "The offering player no longer has those resources",
            RuleError::NoSuchPlayer => "No such player",
            RuleError::ManaEmpty => "Your mana is still recharging",
            RuleError::SpellTarget => "That spell cannot be cast there",
            RuleError::WindfallPending => "Windfall already waits for your next roll",
            RuleError::SettlementTooClose => "Towns and castles can't stand next to another town or castle",
            RuleError::Blockading => "That knight is holding a blockade",
            RuleError::NotBlockading => "That knight has no blockade to lift",
        })
    }
}

impl std::error::Error for RuleError {}

/// Something that happened: the history players read, and what clients animate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    TurnStarted {
        player: PlayerId,
        round: u32,
    },
    Rolled {
        player: PlayerId,
        dice: (u8, u8),
    },
    Produced {
        player: PlayerId,
        tile: TileId,
        resource: Resource,
        amount: u32,
    },
    ProductionBlocked {
        tile: TileId,
    },
    SpellCast {
        player: PlayerId,
        spell: Spell,
        tiles: Vec<TileId>,
    },
    /// A tile's number came up under frost: it produced nothing.
    ProductionStopped {
        tile: TileId,
        spell: Spell,
    },
    /// A lingering spell used its last roll or round.
    SpellFaded {
        tile: TileId,
        spell: Spell,
    },
    WindfallPaid {
        player: PlayerId,
        gained: Resources,
    },
    /// Something appeared on a tile; what, nobody knows until a knight finds it.
    ItemAppeared {
        tile: TileId,
    },
    /// A knight walked onto an item: `gained` resources, the scroll's
    /// spell, or `count` relics.
    ItemFound {
        player: PlayerId,
        tile: TileId,
        kind: ItemKind,
        count: u32,
        gained: Resources,
        scroll: Option<Spell>,
    },
    /// An item lay unclaimed too long.
    ItemVanished {
        tile: TileId,
    },
    /// A robber landed on an item and took it.
    ItemStolen {
        tile: TileId,
    },
    /// A fallen knight's relics lie where it fell.
    RelicDropped {
        player: PlayerId,
        tile: TileId,
        count: u32,
    },
    /// A knight blockades its tile: it produces nothing.
    BlockadeLaid {
        player: PlayerId,
        unit: UnitId,
        tile: TileId,
    },
    /// The knight packs up; the blockade holds until the end of round
    /// `free_after`.
    BlockadeLifting {
        player: PlayerId,
        unit: UnitId,
        tile: TileId,
        free_after: u32,
    },
    /// The blockade is gone: the knight is free and the tile produces again.
    BlockadeEnded {
        player: PlayerId,
        unit: UnitId,
        tile: TileId,
    },
    /// A tile's number came up under a knight's blockade: it produced nothing.
    ProductionBlockaded {
        player: PlayerId,
        tile: TileId,
    },
    /// A tile the player rings with roads and buildings paid them its resource.
    RingPaid {
        player: PlayerId,
        tile: TileId,
        resource: Resource,
        amount: u32,
    },
    BuiltRoad {
        player: PlayerId,
        edge: EdgeId,
    },
    /// A new building (always an outpost, except the starting town).
    Built {
        player: PlayerId,
        vertex: VertexId,
        kind: BuildingKind,
    },
    Upgraded {
        player: PlayerId,
        vertex: VertexId,
        to: BuildingKind,
    },
    CardBought {
        player: PlayerId,
        kind: UnitKind,
    },
    Deployed {
        player: PlayerId,
        unit: UnitId,
        kind: UnitKind,
        tile: TileId,
    },
    UnitMoved {
        player: PlayerId,
        unit: UnitId,
        kind: UnitKind,
        from: TileId,
        to: TileId,
    },
    KnightKilledRobber {
        player: PlayerId,
        unit: UnitId,
        robber: RobberId,
        tile: TileId,
    },
    KnightsClashed {
        attacker: PlayerId,
        defender: PlayerId,
        tiles: (TileId, TileId),
    },
    RobberRelocated {
        player: PlayerId,
        robber: RobberId,
        from: TileId,
        to: TileId,
    },
    RobberWandered {
        robber: RobberId,
        from: TileId,
        to: TileId,
    },
    RobberSpawned {
        robber: RobberId,
        tile: TileId,
    },
    /// The roller of a 7 put a new robber on the board.
    RobberPlaced {
        player: PlayerId,
        robber: RobberId,
        tile: TileId,
    },
    TradeProposed {
        from: PlayerId,
        to: Option<PlayerId>,
        give: Resources,
        want: Resources,
    },
    TradeCancelled {
        from: PlayerId,
    },
    TradeDeclined {
        player: PlayerId,
    },
    Traded {
        from: PlayerId,
        to: PlayerId,
        give: Resources,
        want: Resources,
    },
    BankTraded {
        player: PlayerId,
        give: Resource,
        get: Resource,
    },
    RoundEnded {
        round: u32,
    },
    Won {
        player: PlayerId,
        points: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedEvent {
    pub seq: u64,
    pub event: Event,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Game {
    pub board: Board,
    pub players: Vec<Player>,
    /// Indexed by vertex.
    pub buildings: Vec<Option<Building>>,
    /// Indexed by edge.
    pub roads: Vec<Option<PlayerId>>,
    pub units: Vec<Unit>,
    pub robbers: Vec<Robber>,
    pub phase: Phase,
    pub current: PlayerId,
    /// Rounds of play, from 1; 0 during setup.
    pub round: u32,
    /// This turn's roll, once rolled.
    pub dice: Option<(u8, u8)>,
    /// The active player rolled a 7 and may still relocate a robber.
    pub robber_relocation: bool,
    /// No robber appears at the end of a round before this one: a knight
    /// cleared one and the island gets a breather.
    #[serde(default)]
    pub robbers_quiet_until: u32,
    /// Spells on tiles, at most one per tile.
    #[serde(default)]
    pub enchantments: Vec<Enchantment>,
    /// A cast Luck's tile, waiting for the next roll.
    #[serde(default)]
    pub luck: Option<TileId>,
    /// Items lying on tiles, at most one per tile.
    #[serde(default)]
    pub items: Vec<Item>,
    /// Victory points that win this match.
    #[serde(default = "classic_goal")]
    pub victory_points: u32,
    pub trade: Option<TradeOffer>,
    pub next_piece_id: u32,
    /// The latest events, oldest first.
    pub events: Vec<LoggedEvent>,
    pub event_seq: u64,
}

impl Game {
    /// A new game for `names` played to `victory_points`, on a generated
    /// board of `map_size`.
    pub fn new(names: &[String], victory_points: u32, map_size: balance::MapSize, rng: &mut Rng) -> Game {
        let mut game = Game::with_board(names, Board::generate(map_size.radius(), rng));
        game.victory_points = victory_points;
        // The island always has a robber, from the first turn on.
        let tiles: Vec<TileId> = game.board.tile_ids().collect();
        if let Some(tile) = rng.pick(&tiles) {
            let id = RobberId(game.next_id());
            game.robbers.push(Robber { id, tile, frozen_through_round: 0 });
        }
        game
    }

    pub fn with_board(names: &[String], board: Board) -> Game {
        assert!((balance::MIN_PLAYERS..=balance::MAX_PLAYERS).contains(&names.len()), "2–4 players");
        let players = names
            .iter()
            .enumerate()
            .map(|(i, name)| Player {
                id: PlayerId(i as u8),
                name: name.clone(),
                resources: balance::STARTING_RESOURCES,
                cards: Cards { knights: balance::STARTING_KNIGHTS },
                mana_ready_round: 0,
                windfall: false,
                scrolls: Vec::new(),
            })
            .collect();
        Game {
            buildings: vec![None; board.vertices.len()],
            roads: vec![None; board.edges.len()],
            board,
            players,
            units: Vec::new(),
            robbers: Vec::new(),
            phase: Phase::Setup { step: 0, placing: SetupPlacing::Building },
            current: PlayerId(0),
            round: 0,
            dice: None,
            robber_relocation: false,
            robbers_quiet_until: 0,
            enchantments: Vec::new(),
            luck: None,
            items: Vec::new(),
            victory_points: classic_goal(),
            trade: None,
            next_piece_id: 1,
            events: Vec::new(),
            event_seq: 0,
        }
    }

    // ----- Queries -----

    /// Turn order of initial placement: forward placing towns, then back
    /// placing outposts, so the last to place a town is first to place an outpost.
    pub fn setup_order(&self) -> Vec<PlayerId> {
        let n = self.players.len() as u8;
        (0..n).chain((0..n).rev()).map(PlayerId).collect()
    }

    /// What the player places at this setup step.
    pub fn setup_building_kind(&self, step: usize) -> BuildingKind {
        if step < self.players.len() { BuildingKind::Town } else { BuildingKind::Outpost }
    }

    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(id.index())
    }

    pub fn building(&self, v: VertexId) -> Option<Building> {
        self.buildings.get(v.index()).copied().flatten()
    }

    pub fn road(&self, e: EdgeId) -> Option<PlayerId> {
        self.roads.get(e.index()).copied().flatten()
    }

    pub fn unit(&self, id: UnitId) -> Option<&Unit> {
        self.units.iter().find(|u| u.id == id)
    }

    pub fn unit_at(&self, tile: TileId) -> Option<&Unit> {
        self.units.iter().find(|u| u.tile == tile)
    }

    pub fn robber(&self, id: RobberId) -> Option<&Robber> {
        self.robbers.iter().find(|r| r.id == id)
    }

    pub fn robber_at(&self, tile: TileId) -> Option<&Robber> {
        self.robbers.iter().find(|r| r.tile == tile)
    }

    /// The spell on `tile`, including a swap with it as the partner.
    pub fn enchantment_at(&self, tile: TileId) -> Option<&Enchantment> {
        self.enchantments.iter().find(|e| e.touches(tile))
    }

    /// A spell lingers somewhere on the board.
    pub fn spells_on_board(&self) -> bool {
        !self.enchantments.is_empty() || self.luck.is_some()
    }

    /// A robber may move onto it: a free tile.
    pub fn robber_may_enter(&self, tile: TileId) -> bool {
        tile.index() < self.board.tiles.len() && !self.occupied(tile)
    }

    /// A player with a city casts spells free.
    pub fn has_city(&self, player: PlayerId) -> bool {
        self.buildings.iter().flatten().any(|b| b.owner == player && b.kind == BuildingKind::City)
    }

    /// What casting `spell` costs `player` (a scroll aside): its price, or
    /// nothing once they have a city.
    pub fn spell_price(&self, player: PlayerId, spell: Spell) -> Resources {
        if self.has_city(player) { Resources::ZERO } else { spell.cost() }
    }

    /// A unit or a robber stands on it.
    pub fn occupied(&self, tile: TileId) -> bool {
        self.unit_at(tile).is_some() || self.robber_at(tile).is_some()
    }

    pub fn item_at(&self, tile: TileId) -> Option<&Item> {
        self.items.iter().find(|i| i.tile == tile)
    }

    /// An item may appear here: a tile with no unit, robber or item.
    pub fn item_may_appear(&self, tile: TileId) -> bool {
        !self.occupied(tile) && self.item_at(tile).is_none()
    }

    /// What players may see: items not yet found keep what they are secret.
    pub fn hide_items(&mut self) {
        for item in self.items.iter_mut().filter(|i| !i.revealed) {
            item.kind = None;
        }
    }

    /// Buildings, and the relics the player's knights carry.
    pub fn points(&self, player: PlayerId) -> u32 {
        let buildings: u32 =
            self.buildings.iter().flatten().filter(|b| b.owner == player).map(|b| b.kind.points()).sum();
        let relics: u32 = self.units.iter().filter(|u| u.owner == player).map(|u| u.relics).sum();
        buildings + relics * balance::RELIC_POINTS
    }

    /// Towns and cities of every player.
    pub fn total_settlements(&self) -> u32 {
        self.buildings.iter().flatten().filter(|b| b.kind.is_settlement()).count() as u32
    }

    /// Robbers the island may hold: one per `SETTLEMENTS_PER_ROBBER` towns
    /// and castles, and one more per `TILES_PER_EXTRA_ROBBER` tiles.
    pub fn robber_capacity(&self) -> u32 {
        self.total_settlements() / balance::SETTLEMENTS_PER_ROBBER
            + self.board.tiles.len() as u32 / balance::TILES_PER_EXTRA_ROBBER
    }

    pub fn winner(&self) -> Option<PlayerId> {
        match self.phase {
            Phase::Finished { winner } => Some(winner),
            _ => None,
        }
    }

    /// The player whose input the game waits for (the active player).
    pub fn is_active(&self, player: PlayerId) -> bool {
        !matches!(self.phase, Phase::Finished { .. }) && self.current == player
    }

    fn touches_own_road(&self, player: PlayerId, v: VertexId) -> bool {
        self.board.vertex(v).edges.iter().any(|&e| self.road(e) == Some(player))
    }

    /// A road here would join the player's network.
    pub fn road_connects(&self, player: PlayerId, e: EdgeId) -> bool {
        self.board
            .edge(e)
            .vertices
            .iter()
            .any(|&v| self.building(v).is_some_and(|b| b.owner == player) || self.touches_own_road(player, v))
    }

    pub fn can_build_road(&self, player: PlayerId, e: EdgeId) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if e.index() >= self.roads.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.road(e).is_some() {
            return Err(RuleError::EdgeTaken);
        }
        if !self.road_connects(player, e) {
            return Err(RuleError::NotConnected);
        }
        self.require_funds(player, &balance::ROAD_COST)
    }

    /// A new building: always an outpost, on an empty vertex at the player's roads.
    pub fn can_build_outpost(&self, player: PlayerId, v: VertexId) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if v.index() >= self.buildings.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.building(v).is_some() {
            return Err(RuleError::VertexTaken);
        }
        if !self.touches_own_road(player, v) {
            return Err(RuleError::NotConnected);
        }
        self.require_funds(player, &balance::OUTPOST_COST)
    }

    /// Raising the player's building on `v` from the level below `to` to `to`.
    pub fn can_upgrade(&self, player: PlayerId, v: VertexId, to: BuildingKind) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        let building = self.building(v).ok_or(RuleError::NotUpgradable)?;
        if building.owner != player {
            return Err(RuleError::NotUpgradable);
        }
        match building.kind.upgrade() {
            Some((next, cost)) if next == to => {
                if self.settlement_nearby(v) {
                    return Err(RuleError::SettlementTooClose);
                }
                self.require_funds(player, &cost)
            }
            _ => Err(RuleError::NotUpgradable),
        }
    }

    /// A town or city stands one side away from `v`: no town or city may
    /// stand on `v` then.
    pub fn settlement_nearby(&self, v: VertexId) -> bool {
        self.board.vertex_neighbors(v).any(|n| self.building(n).is_some_and(|b| b.kind.is_settlement()))
    }

    pub fn can_buy_card(&self, player: PlayerId, kind: UnitKind) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        self.require_funds(player, &kind.card_cost())
    }

    /// Unoccupied tiles without an item touching one of the player's towns
    /// or cities.
    pub fn deploy_tiles(&self, player: PlayerId) -> Vec<TileId> {
        let mut tiles: Vec<TileId> = self
            .board
            .vertices
            .iter()
            .filter(|v| self.building(v.id).is_some_and(|b| b.owner == player && b.kind.is_settlement()))
            .flat_map(|v| v.tiles.iter().copied())
            .filter(|&t| !self.occupied(t) && self.item_at(t).is_none())
            .collect();
        tiles.sort();
        tiles.dedup();
        tiles
    }

    pub fn can_deploy(&self, player: PlayerId, kind: UnitKind, tile: TileId) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if self.players[player.index()].cards.get(kind) == 0 {
            return Err(RuleError::NoCard);
        }
        if tile.index() >= self.board.tiles.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.occupied(tile) || self.item_at(tile).is_some() {
            return Err(RuleError::TileOccupied);
        }
        if !self.deploy_tiles(player).contains(&tile) {
            return Err(RuleError::NotNextToYourTown);
        }
        Ok(())
    }

    fn ready_unit(&self, player: PlayerId, id: UnitId) -> Result<&Unit, RuleError> {
        self.require_main_action(player)?;
        let unit = self.unit(id).ok_or(RuleError::NoSuchUnit)?;
        if unit.owner != player {
            return Err(RuleError::NotYourUnit);
        }
        if !unit.ready {
            return Err(RuleError::UnitExhausted);
        }
        Ok(unit)
    }

    pub fn can_move(&self, player: PlayerId, id: UnitId, to: TileId) -> Result<(), RuleError> {
        let unit = self.free_unit(player, id)?;
        if to.index() >= self.board.tiles.len() || !self.board.are_adjacent(unit.tile, to) {
            return Err(RuleError::NotAdjacent);
        }
        if self.occupied(to) {
            return Err(RuleError::TileOccupied);
        }
        Ok(())
    }

    /// A ready unit of the player's that holds no blockade.
    fn free_unit(&self, player: PlayerId, id: UnitId) -> Result<&Unit, RuleError> {
        let unit = self.ready_unit(player, id)?;
        if unit.blockade.is_some() {
            return Err(RuleError::Blockading);
        }
        Ok(unit)
    }

    pub fn can_lay_blockade(&self, player: PlayerId, id: UnitId) -> Result<(), RuleError> {
        self.free_unit(player, id).map(|_| ())
    }

    pub fn can_lift_blockade(&self, player: PlayerId, id: UnitId) -> Result<(), RuleError> {
        let unit = self.ready_unit(player, id)?;
        if unit.blockade != Some(Blockade::Holding) {
            return Err(RuleError::NotBlockading);
        }
        Ok(())
    }

    /// A knight's blockade stops the tile producing.
    pub fn blockaded(&self, tile: TileId) -> bool {
        self.unit_at(tile).is_some_and(|u| u.blockade.is_some())
    }

    /// Tiles the player rings: their roads on all six sides and their
    /// buildings on all six corners.
    pub fn ringed_tiles(&self, player: PlayerId) -> Vec<TileId> {
        self.board
            .tiles
            .iter()
            .filter(|t| {
                t.edges.iter().all(|&e| self.road(e) == Some(player))
                    && t.vertices.iter().all(|&v| self.building(v).is_some_and(|b| b.owner == player))
            })
            .map(|t| t.id)
            .collect()
    }

    /// A city stands on one of the tile's corners: a ring around it pays.
    pub fn city_on(&self, tile: TileId) -> bool {
        self.board.tile(tile).vertices.iter().any(|&v| self.building(v).is_some_and(|b| b.kind == BuildingKind::City))
    }

    /// Tiles the unit may move to now.
    pub fn move_targets(&self, player: PlayerId, id: UnitId) -> Vec<TileId> {
        let Some(unit) = self.unit(id) else {
            return Vec::new();
        };
        self.board.tile(unit.tile).neighbors.iter().copied().filter(|&t| self.can_move(player, id, t).is_ok()).collect()
    }

    pub fn can_attack(&self, player: PlayerId, id: UnitId, target: Target) -> Result<(), RuleError> {
        let unit = self.free_unit(player, id)?;
        match target {
            Target::Robber(r) => {
                let robber = self.robber(r).ok_or(RuleError::NoSuchRobber)?;
                if !self.board.are_adjacent(unit.tile, robber.tile) {
                    return Err(RuleError::NotAdjacent);
                }
                Ok(())
            }
            Target::Unit(u) => {
                let other = self.unit(u).ok_or(RuleError::NoSuchUnit)?;
                if other.owner == player {
                    return Err(RuleError::InvalidTarget);
                }
                if !self.board.are_adjacent(unit.tile, other.tile) {
                    return Err(RuleError::NotAdjacent);
                }
                Ok(())
            }
        }
    }

    /// Everything the unit may attack now.
    pub fn attack_targets(&self, player: PlayerId, id: UnitId) -> Vec<Target> {
        let Some(unit) = self.unit(id) else {
            return Vec::new();
        };
        let near = &self.board.tile(unit.tile).neighbors;
        let candidates: Vec<Target> = self
            .robbers
            .iter()
            .filter(|r| near.contains(&r.tile))
            .map(|r| Target::Robber(r.id))
            .chain(self.units.iter().filter(|u| near.contains(&u.tile)).map(|u| Target::Unit(u.id)))
            .collect();
        candidates.into_iter().filter(|&t| self.can_attack(player, id, t).is_ok()).collect()
    }

    pub fn can_relocate_robber(&self, player: PlayerId, robber: RobberId, to: TileId) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if !self.robber_relocation {
            return Err(RuleError::NoRobberRelocation);
        }
        self.robber(robber).ok_or(RuleError::NoSuchRobber)?;
        if to.index() >= self.board.tiles.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.occupied(to) {
            return Err(RuleError::TileOccupied);
        }
        Ok(())
    }

    /// After a 7 with no robber on the board, the roller places a new one.
    pub fn can_place_robber(&self, player: PlayerId, to: TileId) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if !self.robber_relocation {
            return Err(RuleError::NoRobberRelocation);
        }
        if !self.robbers.is_empty() {
            return Err(RuleError::RobberToMove);
        }
        if to.index() >= self.board.tiles.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.occupied(to) {
            return Err(RuleError::TileOccupied);
        }
        Ok(())
    }

    /// Setup: where the player may place now.
    pub fn setup_vertices(&self, player: PlayerId) -> Vec<VertexId> {
        match self.phase {
            Phase::Setup { step, placing: SetupPlacing::Building } if self.current == player => {
                let town = self.setup_building_kind(step).is_settlement();
                self.board
                    .vertices
                    .iter()
                    .map(|v| v.id)
                    .filter(|&v| self.building(v).is_none() && !(town && self.settlement_nearby(v)))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    pub fn setup_edges(&self, player: PlayerId) -> Vec<EdgeId> {
        match self.phase {
            Phase::Setup { placing: SetupPlacing::Road { from }, .. } if self.current == player => {
                self.board.vertex(from).edges.iter().copied().filter(|&e| self.road(e).is_none()).collect()
            }
            _ => Vec::new(),
        }
    }

    fn require_main_action(&self, player: PlayerId) -> Result<(), RuleError> {
        self.require_turn(player)?;
        if self.dice.is_none() {
            return Err(RuleError::MustRollFirst);
        }
        Ok(())
    }

    fn require_turn(&self, player: PlayerId) -> Result<(), RuleError> {
        match self.phase {
            Phase::Finished { .. } => Err(RuleError::GameOver),
            Phase::Setup { .. } => Err(RuleError::SetupInProgress),
            Phase::Playing if self.current != player => Err(RuleError::NotYourTurn),
            Phase::Playing => Ok(()),
        }
    }

    fn require_funds(&self, player: PlayerId, cost: &Resources) -> Result<(), RuleError> {
        if self.players[player.index()].resources.covers(cost) { Ok(()) } else { Err(RuleError::CannotAfford) }
    }

    /// Casting `spell` on `tiles` (as many as it targets): needs full mana,
    /// the spell's price (nothing with a city) and fitting targets, or else
    /// a scroll of it and fitting targets. A lingering spell replaces any
    /// already on its tiles.
    pub fn can_cast(&self, player: PlayerId, spell: Spell, tiles: &[TileId]) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        let scroll = self.players[player.index()].scrolls.contains(&spell);
        if !scroll && self.round < self.players[player.index()].mana_ready_round {
            return Err(RuleError::ManaEmpty);
        }
        if tiles.len() != spell.targets() || tiles.iter().any(|t| t.index() >= self.board.tiles.len()) {
            return Err(RuleError::InvalidTarget);
        }
        match spell {
            Spell::Lure => {
                self.robber_at(tiles[0]).ok_or(RuleError::NoSuchRobber)?;
                if self.occupied(tiles[1]) {
                    return Err(RuleError::TileOccupied);
                }
            }
            Spell::Swap => {
                if self.board.tile(tiles[0]).number == self.board.tile(tiles[1]).number {
                    return Err(RuleError::SpellTarget);
                }
            }
            Spell::Windfall if self.players[player.index()].windfall => return Err(RuleError::WindfallPending),
            _ => {}
        }
        if scroll { Ok(()) } else { self.require_funds(player, &self.spell_price(player, spell)) }
    }

    /// A scroll of the spell is used up first: free, and the mana stays.
    fn cast(&mut self, player: PlayerId, spell: Spell, tiles: &[TileId]) -> Result<(), RuleError> {
        self.can_cast(player, spell, tiles)?;
        let scrolls = &mut self.players[player.index()].scrolls;
        if let Some(i) = scrolls.iter().position(|&s| s == spell) {
            scrolls.remove(i);
        } else {
            self.pay(player, &self.spell_price(player, spell));
            self.players[player.index()].mana_ready_round = self.round + balance::MANA_RECHARGE_ROUNDS;
        }
        self.log(Event::SpellCast { player, spell, tiles: tiles.to_vec() });
        if let Some(charges) = spell.lasting() {
            for &t in tiles {
                while let Some(i) = self.enchantments.iter().position(|e| e.touches(t)) {
                    self.lift(i);
                }
            }
            let partner = (spell == Spell::Swap).then(|| tiles[1]);
            if let Some(b) = partner {
                let a = tiles[0];
                let (na, nb) = (self.board.tile(a).number, self.board.tile(b).number);
                self.board.tiles[a.index()].number = nb;
                self.board.tiles[b.index()].number = na;
            }
            self.enchantments.push(Enchantment { tile: tiles[0], spell, caster: player, charges, partner });
        }
        match spell {
            Spell::Lure => {
                let (from, to) = (tiles[0], tiles[1]);
                let round = self.round;
                let r = self.robbers.iter_mut().find(|r| r.tile == from).expect("checked");
                r.tile = to;
                r.frozen_through_round = round;
                let robber = r.id;
                self.log(Event::RobberRelocated { player, robber, from, to });
                self.robber_landed(to);
            }
            Spell::Windfall => self.players[player.index()].windfall = true,
            Spell::Luck => self.luck = Some(tiles[0]),
            _ => {}
        }
        Ok(())
    }

    /// Ends the spell at `index`: a swap gives the tiles their numbers back.
    fn lift(&mut self, index: usize) {
        let e = self.enchantments.remove(index);
        if let Some(b) = e.partner {
            let (na, nb) = (self.board.tile(e.tile).number, self.board.tile(b).number);
            self.board.tiles[e.tile.index()].number = nb;
            self.board.tiles[b.index()].number = na;
        }
        self.log(Event::SpellFaded { tile: e.tile, spell: e.spell });
    }

    /// Trading `balance::BANK_TRADE_RATE` of `give` for one `get` with the bank.
    pub fn can_trade_with_bank(&self, player: PlayerId, give: Resource, get: Resource) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if give == get {
            return Err(RuleError::InvalidTrade);
        }
        let mut cost = Resources::ZERO;
        *cost.get_mut(give) = balance::BANK_TRADE_RATE;
        self.require_funds(player, &cost)
    }

    // ----- Actions -----

    /// Validates `action` from `player` and executes it, or leaves the game
    /// untouched and says why not.
    pub fn apply(&mut self, player: PlayerId, action: Action, rng: &mut Rng) -> Result<(), RuleError> {
        if player.index() >= self.players.len() {
            return Err(RuleError::NoSuchPlayer);
        }
        match action {
            Action::SetupBuilding(v) => self.setup_building(player, v),
            Action::SetupRoad(e) => self.setup_road(player, e, rng),
            Action::Roll => self.roll(player, rng),
            Action::BuildRoad(e) => {
                self.can_build_road(player, e)?;
                self.pay(player, &balance::ROAD_COST);
                self.roads[e.index()] = Some(player);
                self.log(Event::BuiltRoad { player, edge: e });
                Ok(())
            }
            Action::BuildOutpost(v) => self.build_outpost(player, v),
            Action::UpgradeTown(v) => self.upgrade(player, v, BuildingKind::Town),
            Action::UpgradeCity(v) => self.upgrade(player, v, BuildingKind::City),
            Action::CastSpell { spell, tiles } => self.cast(player, spell, &tiles),
            Action::BuyCard(kind) => {
                self.can_buy_card(player, kind)?;
                self.pay(player, &kind.card_cost());
                *self.players[player.index()].cards.get_mut(kind) += 1;
                self.log(Event::CardBought { player, kind });
                Ok(())
            }
            Action::Deploy { kind, tile } => {
                self.can_deploy(player, kind, tile)?;
                *self.players[player.index()].cards.get_mut(kind) -= 1;
                let id = UnitId(self.next_id());
                self.units.push(Unit { id, owner: player, kind, tile, ready: false, relics: 0, blockade: None });
                self.log(Event::Deployed { player, unit: id, kind, tile });
                Ok(())
            }
            Action::MoveUnit { unit, to } => {
                self.can_move(player, unit, to)?;
                let u = self.units.iter_mut().find(|u| u.id == unit).expect("checked");
                let from = u.tile;
                u.tile = to;
                u.ready = false;
                let kind = u.kind;
                self.log(Event::UnitMoved { player, unit, kind, from, to });
                self.pick_up(player, unit, to, rng);
                Ok(())
            }
            Action::Attack { unit, target } => self.attack(player, unit, target),
            Action::RelocateRobber { robber, to } => {
                self.can_relocate_robber(player, robber, to)?;
                let round = self.round;
                let r = self.robbers.iter_mut().find(|r| r.id == robber).expect("checked");
                let from = r.tile;
                r.tile = to;
                r.frozen_through_round = round;
                self.robber_relocation = false;
                self.log(Event::RobberRelocated { player, robber, from, to });
                self.robber_landed(to);
                Ok(())
            }
            Action::PlaceRobber { to } => {
                self.can_place_robber(player, to)?;
                let robber = RobberId(self.next_id());
                self.robbers.push(Robber { id: robber, tile: to, frozen_through_round: self.round });
                self.robber_relocation = false;
                self.log(Event::RobberPlaced { player, robber, tile: to });
                self.robber_landed(to);
                Ok(())
            }
            Action::ProposeTrade { to, give, want } => {
                self.require_main_action(player)?;
                if self.trade.is_some() {
                    return Err(RuleError::TradeAlreadyOpen);
                }
                if give.is_empty() || want.is_empty() || to == Some(player) {
                    return Err(RuleError::InvalidTrade);
                }
                if to.is_some_and(|p| p.index() >= self.players.len()) {
                    return Err(RuleError::NoSuchPlayer);
                }
                self.require_funds(player, &give)?;
                self.trade = Some(TradeOffer { from: player, to, give, want, declined: Vec::new() });
                self.log(Event::TradeProposed { from: player, to, give, want });
                Ok(())
            }
            Action::TradeWithBank { give, get } => {
                self.can_trade_with_bank(player, give, get)?;
                let r = &mut self.players[player.index()].resources;
                *r.get_mut(give) -= balance::BANK_TRADE_RATE;
                *r.get_mut(get) += 1;
                self.log(Event::BankTraded { player, give, get });
                Ok(())
            }
            Action::CancelTrade => {
                self.require_turn(player)?;
                if self.trade.take().is_none() {
                    return Err(RuleError::NoTrade);
                }
                self.log(Event::TradeCancelled { from: player });
                Ok(())
            }
            Action::AcceptTrade => {
                let offer = self.trade_for(player)?.clone();
                if !self.players[offer.from.index()].resources.covers(&offer.give) {
                    return Err(RuleError::TraderCannotPay);
                }
                self.require_funds(player, &offer.want)?;
                let (from, to) = (offer.from.index(), player.index());
                self.players[from].resources.remove(&offer.give);
                self.players[to].resources.add(&offer.give);
                self.players[to].resources.remove(&offer.want);
                self.players[from].resources.add(&offer.want);
                self.trade = None;
                self.log(Event::Traded { from: offer.from, to: player, give: offer.give, want: offer.want });
                Ok(())
            }
            Action::DeclineTrade => {
                self.trade_for(player)?;
                let others = self.players.len() - 1;
                let offer = self.trade.as_mut().expect("checked");
                offer.declined.push(player);
                if offer.to.is_some() || offer.declined.len() >= others {
                    self.trade = None;
                }
                self.log(Event::TradeDeclined { player });
                Ok(())
            }
            Action::LayBlockade { unit } => {
                self.can_lay_blockade(player, unit)?;
                let u = self.units.iter_mut().find(|u| u.id == unit).expect("checked");
                u.blockade = Some(Blockade::Holding);
                u.ready = false;
                let tile = u.tile;
                self.log(Event::BlockadeLaid { player, unit, tile });
                Ok(())
            }
            Action::LiftBlockade { unit } => {
                self.can_lift_blockade(player, unit)?;
                let free_after = self.round + balance::BLOCKADE_LIFT_ROUNDS;
                let u = self.units.iter_mut().find(|u| u.id == unit).expect("checked");
                u.blockade = Some(Blockade::Lifting { free_after });
                u.ready = false;
                let tile = u.tile;
                self.log(Event::BlockadeLifting { player, unit, tile, free_after });
                Ok(())
            }
            Action::EndTurn => {
                self.require_main_action(player)?;
                self.end_turn(rng);
                Ok(())
            }
        }
    }

    /// The open offer `player` may answer.
    pub(crate) fn trade_for(&self, player: PlayerId) -> Result<&TradeOffer, RuleError> {
        if matches!(self.phase, Phase::Finished { .. }) {
            return Err(RuleError::GameOver);
        }
        let offer = self.trade.as_ref().ok_or(RuleError::NoTrade)?;
        if offer.from == player || offer.to.is_some_and(|p| p != player) || offer.declined.contains(&player) {
            return Err(RuleError::NotTradeRecipient);
        }
        Ok(offer)
    }

    fn setup_building(&mut self, player: PlayerId, v: VertexId) -> Result<(), RuleError> {
        let Phase::Setup { step, placing } = self.phase else {
            return Err(RuleError::NotSetup);
        };
        if self.current != player {
            return Err(RuleError::NotYourTurn);
        }
        if placing != SetupPlacing::Building {
            return Err(RuleError::InvalidTarget);
        }
        if v.index() >= self.buildings.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.building(v).is_some() {
            return Err(RuleError::VertexTaken);
        }
        let kind = self.setup_building_kind(step);
        if kind.is_settlement() && self.settlement_nearby(v) {
            return Err(RuleError::SettlementTooClose);
        }
        self.buildings[v.index()] = Some(Building { owner: player, kind });
        self.phase = Phase::Setup { step, placing: SetupPlacing::Road { from: v } };
        self.log(Event::Built { player, vertex: v, kind });
        Ok(())
    }

    fn setup_road(&mut self, player: PlayerId, e: EdgeId, rng: &mut Rng) -> Result<(), RuleError> {
        let Phase::Setup { step, placing } = self.phase else {
            return Err(RuleError::NotSetup);
        };
        if self.current != player {
            return Err(RuleError::NotYourTurn);
        }
        let SetupPlacing::Road { from } = placing else {
            return Err(RuleError::InvalidTarget);
        };
        if e.index() >= self.roads.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.road(e).is_some() {
            return Err(RuleError::EdgeTaken);
        }
        if !self.board.edge(e).vertices.contains(&from) {
            return Err(RuleError::NotConnected);
        }
        self.roads[e.index()] = Some(player);
        self.log(Event::BuiltRoad { player, edge: e });
        let order = self.setup_order();
        let next = step + 1;
        if next < order.len() {
            self.phase = Phase::Setup { step: next, placing: SetupPlacing::Building };
            self.current = order[next];
        } else {
            // Before round 1 starts, so they last `ITEM_ROUNDS` rounds like
            // those left at the end of a round.
            self.top_up_items(0, rng);
            self.phase = Phase::Playing;
            self.round = 1;
            self.begin_turn(PlayerId(0));
        }
        Ok(())
    }

    fn roll(&mut self, player: PlayerId, rng: &mut Rng) -> Result<(), RuleError> {
        self.require_turn(player)?;
        if self.dice.is_some() {
            return Err(RuleError::AlreadyRolled);
        }
        let dice = self.dice_for_roll(rng);
        self.dice = Some(dice);
        self.log(Event::Rolled { player, dice });
        if let Some(tile) = self.luck.take() {
            self.log(Event::SpellFaded { tile, spell: Spell::Luck });
        }
        let total = dice.0 + dice.1;
        if total == balance::ROBBER_ROLL {
            // Move a robber, or place one when the board has none.
            self.robber_relocation = true;
            self.pay_windfall(player);
            return Ok(());
        }
        self.produce(total);
        self.pay_windfall(player);
        Ok(())
    }

    /// Two dice, unless a Luck waits: then its tile's number comes up with
    /// exactly `balance::LUCK_CHANCE`, and otherwise any other total.
    fn dice_for_roll(&self, rng: &mut Rng) -> (u8, u8) {
        let roll = |rng: &mut Rng| (rng.die(), rng.die());
        let Some(tile) = self.luck else { return roll(rng) };
        let number = self.board.tile(tile).number;
        let (num, den) = balance::LUCK_CHANCE;
        if rng.chance(num, den) {
            let pairs: Vec<(u8, u8)> =
                (1..=6).flat_map(|a| (1..=6).map(move |b| (a, b))).filter(|&(a, b)| a + b == number).collect();
            if let Some(dice) = rng.pick(&pairs) {
                return dice;
            }
        }
        loop {
            let dice = roll(rng);
            if dice.0 + dice.1 != number {
                return dice;
            }
        }
    }

    /// A pending windfall: one of each resource the player's buildings touch.
    fn pay_windfall(&mut self, player: PlayerId) {
        if !std::mem::take(&mut self.players[player.index()].windfall) {
            return;
        }
        let mut gained = Resources::ZERO;
        for (v, b) in self.buildings.iter().enumerate() {
            if b.is_some_and(|b| b.owner == player) {
                for &t in &self.board.vertices[v].tiles {
                    *gained.get_mut(self.board.tile(t).resource) = 1;
                }
            }
        }
        let r = &mut self.players[player.index()].resources;
        for res in Resource::ALL {
            *r.get_mut(res) += gained.get(res);
        }
        self.log(Event::WindfallPaid { player, gained });
    }

    /// Every building on a tile showing `number` collects its resource,
    /// unless a bandit or a knight's blockade stands there. Frost stops it;
    /// bounty doubles it. Each roll of the number uses one of the tile's
    /// spell charges.
    pub fn produce(&mut self, number: u8) {
        let tiles: Vec<TileId> = self.board.tiles.iter().filter(|t| t.number == number).map(|t| t.id).collect();
        for tile in tiles {
            let spell = self.enchantment_at(tile).map(|e| e.spell);
            if self.robber_at(tile).is_some() {
                self.log(Event::ProductionBlocked { tile });
            } else if let Some(u) = self.unit_at(tile).filter(|u| u.blockade.is_some()) {
                let player = u.owner;
                self.log(Event::ProductionBlockaded { player, tile });
            } else if let Some(spell) = spell.filter(|s| s.stops()) {
                self.log(Event::ProductionStopped { tile, spell });
            } else {
                let factor = if spell.is_some_and(Spell::doubles) { 2 } else { 1 };
                let resource = self.board.tile(tile).resource;
                let mut gains = vec![0u32; self.players.len()];
                for &v in &self.board.tile(tile).vertices {
                    if let Some(b) = self.building(v) {
                        gains[b.owner.index()] += b.kind.yield_per_tile() * factor;
                    }
                }
                for (i, amount) in gains.into_iter().enumerate() {
                    if amount > 0 {
                        *self.players[i].resources.get_mut(resource) += amount;
                        self.log(Event::Produced { player: PlayerId(i as u8), tile, resource, amount });
                    }
                }
            }
            self.spend_charge(tile);
        }
    }

    /// The spell on `tile` used a charge; on its last it fades.
    fn spend_charge(&mut self, tile: TileId) {
        let found = self.enchantments.iter().position(|e| e.touches(tile));
        let Some(i) = found else { return };
        self.enchantments[i].charges -= 1;
        if self.enchantments[i].charges == 0 {
            self.lift(i);
        }
    }

    fn build_outpost(&mut self, player: PlayerId, v: VertexId) -> Result<(), RuleError> {
        self.can_build_outpost(player, v)?;
        self.pay(player, &balance::OUTPOST_COST);
        self.buildings[v.index()] = Some(Building { owner: player, kind: BuildingKind::Outpost });
        self.log(Event::Built { player, vertex: v, kind: BuildingKind::Outpost });
        Ok(())
    }

    fn upgrade(&mut self, player: PlayerId, v: VertexId, to: BuildingKind) -> Result<(), RuleError> {
        self.can_upgrade(player, v, to)?;
        let (_, cost) = self.building(v).and_then(|b| b.kind.upgrade()).expect("checked");
        self.pay(player, &cost);
        self.buildings[v.index()] = Some(Building { owner: player, kind: to });
        self.log(Event::Upgraded { player, vertex: v, to });
        self.check_victory(player);
        Ok(())
    }

    fn attack(&mut self, player: PlayerId, id: UnitId, target: Target) -> Result<(), RuleError> {
        self.can_attack(player, id, target)?;
        let attacker = self.unit(id).expect("checked").clone();
        match target {
            Target::Robber(r) => {
                // The knight falls with the robber it brings down.
                let tile = self.robber(r).expect("checked").tile;
                self.robbers.retain(|x| x.id != r);
                self.robbers_quiet_until = self.round + balance::ROBBER_RESPAWN_ROUNDS;
                self.log(Event::KnightKilledRobber { player, unit: id, robber: r, tile });
                self.fall(id);
            }
            Target::Unit(u) => {
                let defender = self.unit(u).expect("checked").clone();
                self.log(Event::KnightsClashed {
                    attacker: player,
                    defender: defender.owner,
                    tiles: (attacker.tile, defender.tile),
                });
                self.fall(u);
                self.fall(id);
            }
        }
        Ok(())
    }

    /// A knight falls: relics it carried drop on its tile; nothing else it
    /// held survives it.
    fn fall(&mut self, id: UnitId) {
        let Some(i) = self.units.iter().position(|u| u.id == id) else { return };
        let unit = self.units.remove(i);
        if unit.relics > 0 {
            self.items.push(Item {
                tile: unit.tile,
                kind: Some(ItemKind::Relic),
                revealed: true,
                count: unit.relics,
                last_round: self.round + balance::ITEM_ROUNDS,
            });
            self.log(Event::RelicDropped { player: unit.owner, tile: unit.tile, count: unit.relics });
        }
    }

    /// A knight walked onto `tile`: its owner takes the item there.
    fn pick_up(&mut self, player: PlayerId, unit: UnitId, tile: TileId, rng: &mut Rng) {
        let Some(i) = self.items.iter().position(|it| it.tile == tile) else { return };
        let item = self.items.remove(i);
        let kind = item.kind.expect("the game itself knows every item");
        let p = player.index();
        let (mut gained, mut scroll) = (Resources::ZERO, None);
        match kind {
            ItemKind::IronOre => gained = balance::IRON_ORE_FIND,
            ItemKind::Supplies => {
                let resource = rng.pick(&Resource::ALL).expect("resources");
                *gained.get_mut(resource) = balance::SUPPLIES_FIND;
            }
            ItemKind::Hoard => gained = balance::HOARD_FIND,
            ItemKind::ManaCrystal => self.players[p].mana_ready_round = self.round,
            ItemKind::Scroll => {
                let spell = rng.pick(&Spell::ALL).expect("spells");
                self.players[p].scrolls.push(spell);
                scroll = Some(spell);
            }
            ItemKind::Recruit => self.players[p].cards.knights += 1,
            ItemKind::Relic => self.units.iter_mut().find(|u| u.id == unit).expect("walked here").relics += item.count,
            ItemKind::Trap => {}
        }
        self.players[p].resources.add(&gained);
        self.log(Event::ItemFound { player, tile, kind, count: item.count, gained, scroll });
        match kind {
            ItemKind::Trap => self.fall(unit),
            ItemKind::Hoard => {
                let near: Vec<TileId> =
                    self.board.tile(tile).neighbors.iter().copied().filter(|&t| self.robber_may_enter(t)).collect();
                if let Some(at) = rng.pick(&near) {
                    let robber = RobberId(self.next_id());
                    self.robbers.push(Robber { id: robber, tile: at, frozen_through_round: 0 });
                    self.log(Event::RobberSpawned { robber, tile: at });
                    self.robber_landed(at);
                }
            }
            ItemKind::Relic => self.check_victory(player),
            _ => {}
        }
    }

    /// A robber came to `tile`: it takes any item there.
    fn robber_landed(&mut self, tile: TileId) {
        if let Some(i) = self.items.iter().position(|it| it.tile == tile) {
            self.items.remove(i);
            self.log(Event::ItemStolen { tile });
        }
    }

    fn end_turn(&mut self, rng: &mut Rng) {
        let next = (self.current.index() + 1) % self.players.len();
        if next == 0 {
            self.end_round(rng);
            self.round += 1;
        }
        self.begin_turn(PlayerId(next as u8));
    }

    /// The player's knights stand ready, and every tile they ring with a
    /// city among its corners pays them, unless a bandit or a blockade
    /// stands on it.
    fn begin_turn(&mut self, player: PlayerId) {
        self.current = player;
        self.dice = None;
        self.robber_relocation = false;
        self.trade = None;
        for unit in self.units.iter_mut().filter(|u| u.owner == player) {
            unit.ready = true;
        }
        self.log(Event::TurnStarted { player, round: self.round });
        for tile in self.ringed_tiles(player) {
            if !self.city_on(tile) || self.robber_at(tile).is_some() || self.blockaded(tile) {
                continue;
            }
            let resource = self.board.tile(tile).resource;
            let amount = balance::RING_INCOME;
            *self.players[player.index()].resources.get_mut(resource) += amount;
            self.log(Event::RingPaid { player, tile, resource, amount });
        }
    }

    /// Robbers wander, then one may spawn if the board is below capacity;
    /// knights packing up long enough are free; old items vanish, then the
    /// island is topped up with hidden items (at least one new one).
    pub fn end_round(&mut self, rng: &mut Rng) {
        let round = self.round;
        for i in 0..self.robbers.len() {
            if self.robbers[i].frozen_through_round >= round {
                continue;
            }
            let from = self.robbers[i].tile;
            let free: Vec<TileId> =
                self.board.tile(from).neighbors.iter().copied().filter(|&t| self.robber_may_enter(t)).collect();
            if let Some(to) = rng.pick(&free) {
                self.robbers[i].tile = to;
                let robber = self.robbers[i].id;
                self.log(Event::RobberWandered { robber, from, to });
                self.robber_landed(to);
            }
        }
        // The island keeps its minimum of robbers, and may gain more while
        // below capacity; none appear during the breather after a kill.
        let count = self.robbers.len() as u32;
        let (num, den) = balance::ROBBER_SPAWN_CHANCE;
        let spawn = round >= self.robbers_quiet_until
            && (count < balance::MIN_ROBBERS || (count < self.robber_capacity() && rng.chance(num, den)));
        if spawn {
            // Far from the other robbers: weighted by the square of the
            // distance to the nearest one.
            let robbers: Vec<TileId> = self.robbers.iter().map(|r| r.tile).collect();
            let distances = self.tile_distances(&robbers);
            let spots: Vec<(TileId, usize)> = self
                .board
                .tile_ids()
                .filter(|&t| self.robber_may_enter(t))
                .map(|t| (t, if robbers.is_empty() { 1 } else { distances[t.index()].pow(2) }))
                .collect();
            if let Some(tile) = pick_weighted(&spots, rng) {
                let id = RobberId(self.next_id());
                self.robbers.push(Robber { id, tile, frozen_through_round: 0 });
                self.log(Event::RobberSpawned { robber: id, tile });
                self.robber_landed(tile);
            }
        }
        for i in 0..self.units.len() {
            if let Some(Blockade::Lifting { free_after }) = self.units[i].blockade
                && free_after <= round
            {
                self.units[i].blockade = None;
                let Unit { owner: player, id: unit, tile, .. } = self.units[i];
                self.log(Event::BlockadeEnded { player, unit, tile });
            }
        }
        let (gone, kept): (Vec<Item>, Vec<Item>) =
            std::mem::take(&mut self.items).into_iter().partition(|i| i.last_round <= round);
        self.items = kept;
        for item in gone {
            self.log(Event::ItemVanished { tile: item.tile });
        }
        // At least one item turns up every round, up to the per-player target.
        self.top_up_items(1, rng);
        self.log(Event::RoundEnded { round });
    }

    /// Hidden items on the island: found and dropped ones do not count.
    fn hidden_items(&self) -> u32 {
        self.items.iter().filter(|i| !i.revealed).count() as u32
    }

    /// Spawns hidden items up to `ITEMS_PER_PLAYER` per player, but at least
    /// `at_least`, each on a distinct eligible tile; stops when none are left.
    /// A tile that is the last one a knight token's holder could deploy
    /// onto stays clear.
    fn top_up_items(&mut self, at_least: u32, rng: &mut Rng) {
        let target = balance::ITEMS_PER_PLAYER * self.players.len() as u32;
        let spawn = target.saturating_sub(self.hidden_items()).max(at_least);
        let weights = self.item_weights();
        for _ in 0..spawn {
            let last_deploy_spots: Vec<TileId> = self
                .players
                .iter()
                .filter(|p| p.cards.knights > 0)
                .filter_map(|p| match self.deploy_tiles(p.id)[..] {
                    [only] => Some(only),
                    _ => None,
                })
                .collect();
            let spots: Vec<(TileId, usize)> = self
                .board
                .tile_ids()
                .filter(|&t| self.item_may_appear(t) && !last_deploy_spots.contains(&t))
                .map(|t| (t, weights[t.index()]))
                .collect();
            let Some(tile) = pick_weighted(&spots, rng) else { break };
            let kind = draw_item(rng);
            let last_round = self.round + balance::ITEM_ROUNDS;
            self.items.push(Item { tile, kind: Some(kind), revealed: false, count: 1, last_round });
            self.log(Event::ItemAppeared { tile });
        }
    }

    /// Per tile, the item placement weight: the distance in tiles to the
    /// nearest knight of any owner, capped at `ITEM_ROUNDS` (every tile
    /// weighs 1 when no knights stand on the island).
    fn item_weights(&self) -> Vec<usize> {
        if self.units.is_empty() {
            return vec![1; self.board.tiles.len()];
        }
        let knights: Vec<TileId> = self.units.iter().map(|u| u.tile).collect();
        let cap = balance::ITEM_ROUNDS as usize;
        self.tile_distances(&knights).into_iter().map(|d| d.min(cap)).collect()
    }

    /// Per tile, the distance in tiles to the nearest of `sources`.
    fn tile_distances(&self, sources: &[TileId]) -> Vec<usize> {
        let mut dist = vec![usize::MAX; self.board.tiles.len()];
        let mut queue = std::collections::VecDeque::new();
        for &t in sources {
            if dist[t.index()] == usize::MAX {
                dist[t.index()] = 0;
                queue.push_back(t);
            }
        }
        while let Some(t) = queue.pop_front() {
            for &nb in &self.board.tile(t).neighbors {
                if dist[nb.index()] == usize::MAX {
                    dist[nb.index()] = dist[t.index()] + 1;
                    queue.push_back(nb);
                }
            }
        }
        dist
    }

    fn check_victory(&mut self, player: PlayerId) {
        let points = self.points(player);
        if points >= self.victory_points {
            self.phase = Phase::Finished { winner: player };
            self.trade = None;
            self.log(Event::Won { player, points });
        }
    }

    fn pay(&mut self, player: PlayerId, cost: &Resources) {
        self.players[player.index()].resources.remove(cost);
    }

    fn next_id(&mut self) -> u32 {
        let id = self.next_piece_id;
        self.next_piece_id += 1;
        id
    }

    fn log(&mut self, event: Event) {
        self.event_seq += 1;
        self.events.push(LoggedEvent { seq: self.event_seq, event });
        if self.events.len() > balance::EVENT_HISTORY {
            let excess = self.events.len() - balance::EVENT_HISTORY;
            self.events.drain(..excess);
        }
    }
}

/// The goal of games saved before goals could be chosen.
fn classic_goal() -> u32 {
    10
}

/// An item kind, as often as `balance::ITEM_WEIGHTS` says.
fn draw_item(rng: &mut Rng) -> ItemKind {
    let total: u32 = balance::ITEM_WEIGHTS.iter().map(|&(_, w)| w).sum();
    let mut n = rng.below(total as usize) as u32;
    for (kind, weight) in balance::ITEM_WEIGHTS {
        if n < weight {
            return kind;
        }
        n -= weight;
    }
    unreachable!("drawn below the total weight")
}

/// A tile drawn with chance proportional to its weight; `None` when every
/// weight is 0 or there are no tiles.
fn pick_weighted(spots: &[(TileId, usize)], rng: &mut Rng) -> Option<TileId> {
    let total: usize = spots.iter().map(|&(_, w)| w).sum();
    if total == 0 {
        return None;
    }
    let mut n = rng.below(total);
    for &(tile, weight) in spots {
        if n < weight {
            return Some(tile);
        }
        n -= weight;
    }
    unreachable!("drawn below the total weight")
}
