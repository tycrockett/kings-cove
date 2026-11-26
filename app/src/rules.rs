//! What every piece, spell and rule does, in words. Numbers come from the
//! engine's `balance` and outcomes from its own rules, so the text cannot
//! drift from the game.

use kings_cove_engine::{BuildingKind, Game, ItemKind, PlayerId, Resources, Spell, Target, UnitId, balance};

use crate::art::Glyph;

/// Something the build tray offers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Road,
    Outpost,
    Town,
    City,
    Knight,
}

impl Item {
    pub const ALL: [Item; 5] = [Item::Road, Item::Outpost, Item::Town, Item::City, Item::Knight];

    pub fn cost(self) -> Resources {
        match self {
            Item::Road => balance::ROAD_COST,
            Item::Outpost => balance::OUTPOST_COST,
            Item::Town => balance::TOWN_UPGRADE_COST,
            Item::City => balance::CITY_UPGRADE_COST,
            Item::Knight => balance::KNIGHT_CARD_COST,
        }
    }

    pub fn glyph(self) -> Glyph {
        match self {
            Item::Road => Glyph::Road,
            Item::Outpost => Glyph::Building(BuildingKind::Outpost),
            Item::Town => Glyph::Building(BuildingKind::Town),
            Item::City => Glyph::Building(BuildingKind::City),
            Item::Knight => Glyph::Knight,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Item::Road => "Road",
            Item::Outpost => "Outpost",
            Item::Town => "Town",
            Item::City => "Castle",
            Item::Knight => "Knight",
        }
    }

    /// The button that buys it.
    pub fn verb(self) -> &'static str {
        match self {
            Item::Road => "Build a road",
            Item::Outpost => "Build an outpost",
            Item::Town => "Upgrade to a town",
            Item::City => "Upgrade to a castle",
            Item::Knight => "Buy a knight card",
        }
    }

    /// Why there is nowhere to use it, when the player can pay but the board
    /// offers no place.
    pub fn no_place(self) -> &'static str {
        match self {
            Item::Road => "No free side touches your roads or buildings",
            Item::Outpost => "No free corner on your roads",
            Item::Town => "No outpost of yours has room: towns need no town or castle next door",
            Item::City => "You have no town to upgrade",
            Item::Knight => "You cannot buy one now",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Item::Road => "Stretches your reach across the island.",
            Item::Outpost => "A new settlement at the end of your roads.",
            Item::Town => "An outpost grown into a town.",
            Item::City => "A town raised into a stone castle.",
            Item::Knight => "A fighter that clears bandits, duels enemy knights and blockades tiles.",
        }
    }

    pub fn rules(self) -> Vec<String> {
        match self {
            Item::Road => vec![
                "Sits on a side of a tile.".to_owned(),
                "Must touch one of your roads or buildings.".to_owned(),
                "New outposts can only go on a corner your roads reach.".to_owned(),
                format!(
                    "Ring a tile with your roads on all six sides and your buildings on all six corners: it pays you {} of its resource at the start of each of your turns, unless a bandit or a blockade stands on it.",
                    balance::RING_INCOME
                ),
            ],
            Item::Outpost => vec![
                "Goes on any free corner one of your roads reaches.".to_owned(),
                format!(
                    "Worth {} VP. Collects {} from each neighbouring tile whose number is rolled.",
                    BuildingKind::Outpost.points(),
                    balance::OUTPOST_YIELD
                ),
                "Upgrade it to a town to score and to deploy knights beside it.".to_owned(),
            ],
            Item::Town => vec![
                "Upgrades one of your outposts in place, unless a town or castle stands on a corner next to it.".to_owned(),
                format!(
                    "Worth {} VP. Collects {} from each neighbouring tile whose number is rolled.",
                    balance::TOWN_POINTS,
                    balance::TOWN_YIELD
                ),
                "Knights deploy onto the tiles it touches.".to_owned(),
            ],
            Item::City => vec![
                "Upgrades one of your towns in place.".to_owned(),
                format!(
                    "Worth {} VP. Collects {} from each neighbouring tile whose number is rolled.",
                    balance::CITY_POINTS,
                    balance::CITY_YIELD
                ),
                "Your first castle unlocks spells.".to_owned(),
            ],
            Item::Knight => vec![
                "Buying gives you a card. Deploy it from your hand onto a free tile touching your town or castle (not an outpost).".to_owned(),
                "Each turn it either moves to a free neighbouring tile or attacks; it rests the turn it arrives.".to_owned(),
                "Attacks a neighbouring bandit or enemy knight:".to_owned(),
                "• Bandit: the bandit and your knight both fall.".to_owned(),
                "• Knight: both knights fall.".to_owned(),
                "Instead of moving or attacking, it can blockade its tile: the tile produces nothing for anyone, and the knight can neither move nor attack.".to_owned(),
                format!(
                    "Lifting the blockade is an order too: the blockade holds for {} more full rounds, then the knight is free.",
                    balance::BLOCKADE_LIFT_ROUNDS
                ),
            ],
        }
    }
}

pub fn spell_title(spell: Spell) -> &'static str {
    match spell {
        Spell::Frost => "Frost",
        Spell::Bounty => "Bounty",
        Spell::Lure => "Lure",
        Spell::Swap => "Swap",
        Spell::Windfall => "Windfall",
        Spell::Luck => "Luck",
    }
}

pub fn spell_summary(spell: Spell) -> &'static str {
    match spell {
        Spell::Frost => "Freezes a tile solid.",
        Spell::Bounty => "Blesses a tile with a double harvest.",
        Spell::Lure => "Draws a bandit wherever you want it.",
        Spell::Swap => "Two tiles trade their numbers.",
        Spell::Windfall => "Your next roll brings a bit of everything.",
        Spell::Luck => "Tilts the next roll toward a tile.",
    }
}

/// What the spell does, then the rules every spell shares.
pub fn spell_rules(spell: Spell) -> Vec<String> {
    let rolls = |effect: &str| {
        format!(
            "Cast on any tile. {effect} for the next {} times its number is rolled, for everyone with a building on it.",
            balance::SPELL_CHARGES
        )
    };
    let mut lines = vec![match spell {
        Spell::Frost => rolls("It produces nothing"),
        Spell::Bounty => rolls("It produces double"),
        Spell::Lure => {
            "Pick a bandit, then any free tile: the bandit moves there and stays put through the next full round."
                .to_owned()
        }
        Spell::Swap => format!(
            "Pick two tiles with different numbers: they trade numbers until either number has been rolled {} times between them.",
            balance::SPELL_CHARGES
        ),
        Spell::Windfall => {
            "On your next roll, whatever comes up, you collect 1 of each resource your buildings touch.".to_owned()
        }
        Spell::Luck => {
            let (num, den) = balance::LUCK_CHANCE;
            format!(
                "Cast on any tile. The next roll, whoever makes it, has a {}% chance of coming up as its number.",
                num * 100 / den
            )
        }
    }];
    if spell.lasting().is_some() {
        lines.push("A spell cast on a tile that already holds one replaces it.".to_owned());
    }
    lines.push(format!(
        "Spells unlock with your first castle. Any spell empties your mana for {} rounds.",
        balance::MANA_RECHARGE_ROUNDS
    ));
    lines
}

/// What a lingering spell's counter counts.
pub fn charges_word(n: u32) -> String {
    format!("{n} roll{} left", if n == 1 { "" } else { "s" })
}

/// What the board asks while a spell is being aimed.
pub fn spell_hint(spell: Spell, picked_first: bool) -> String {
    match (spell, picked_first) {
        (Spell::Frost | Spell::Bounty | Spell::Luck, _) => format!("Tap a tile for {}", spell.name()),
        (Spell::Lure, false) => "Tap the bandit to lure".to_owned(),
        (Spell::Lure, true) => "Tap a free tile to lure it to".to_owned(),
        (Spell::Swap, false) => "Tap the first tile to swap".to_owned(),
        (Spell::Swap, true) => "Tap a tile with another number to swap with".to_owned(),
        (Spell::Windfall, _) => String::new(),
    }
}

pub fn item_title(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::IronOre => "Iron ore",
        ItemKind::Supplies => "Supplies",
        ItemKind::ManaCrystal => "Mana crystal",
        ItemKind::Scroll => "Spell scroll",
        ItemKind::Recruit => "Recruit",
        ItemKind::Relic => "Relic",
        ItemKind::Trap => "Trap",
        ItemKind::Hoard => "Bandit's hoard",
    }
}

/// What an item does when a knight finds it.
pub fn item_rule(kind: ItemKind) -> String {
    match kind {
        ItemKind::IronOre => format!("{}.", balance::IRON_ORE_FIND),
        ItemKind::Supplies => format!("{} of one resource, whichever turns up.", balance::SUPPLIES_FIND),
        ItemKind::ManaCrystal => "Your mana refills at once.".to_owned(),
        ItemKind::Scroll => {
            "One free cast of a random spell: no price, no castle needed, and your mana stays as it is.".to_owned()
        }
        ItemKind::Recruit => "A knight card.".to_owned(),
        ItemKind::Relic => format!(
            "The knight carries it, worth {} VP to you. If the knight falls, the relic drops where it fell.",
            balance::RELIC_POINTS
        ),
        ItemKind::Trap => "The knight falls.".to_owned(),
        ItemKind::Hoard => format!("{}, but a bandit appears next to it.", balance::HOARD_FIND),
    }
}

/// What finding an item gave, in a few words.
pub fn item_found(kind: ItemKind, count: u32, gained: &Resources, scroll: Option<Spell>) -> String {
    match kind {
        ItemKind::IronOre | ItemKind::Supplies | ItemKind::Hoard => format!("{}: {gained}", item_title(kind)),
        ItemKind::ManaCrystal => "Mana crystal: mana refilled".to_owned(),
        ItemKind::Scroll => format!("Scroll: a free {}", scroll.map_or("spell", Spell::name)),
        ItemKind::Recruit => "Recruit: a knight card".to_owned(),
        ItemKind::Relic => format!("Relic: +{} VP", count * balance::RELIC_POINTS),
        ItemKind::Trap => "A trap! The knight fell".to_owned(),
    }
}

/// A page of the rulebook.
pub struct Section {
    pub title: String,
    pub glyph: Option<Glyph>,
    pub cost: Option<Resources>,
    pub lines: Vec<String>,
}

/// The goals a match may be played to, as "10, 20 or 30".
pub fn goals_text() -> String {
    let goals: Vec<String> = balance::VICTORY_POINT_GOALS.iter().map(u32::to_string).collect();
    match goals.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => goals.join(""),
    }
}

/// The rulebook, for a match played to `goal` when there is one.
pub fn rulebook(goal: Option<u32>) -> Vec<Section> {
    let page = |title: &str, glyph, lines: Vec<String>| Section { title: title.to_owned(), glyph, cost: None, lines };
    let (spawn_num, spawn_den) = balance::ROBBER_SPAWN_CHANCE;
    let start = balance::STARTING_RESOURCES;
    let mut pages = vec![
        page(
            "Goal",
            Some(Glyph::Building(BuildingKind::City)),
            vec![
                match goal {
                    Some(goal) => format!("The first player to {goal} victory points wins."),
                    None => format!(
                        "The first player to the match's goal wins: {} victory points, chosen when it is created along with the island's size: {}.",
                        goals_text(),
                        balance::MapSize::ALL
                            .iter()
                            .map(|s| format!("{} ({} tiles)", s.label().to_lowercase(), s.tiles()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                },
                format!(
                    "Towns are worth {} VP and castles {}; outposts score nothing.",
                    balance::TOWN_POINTS,
                    balance::CITY_POINTS
                ),
            ],
        ),
        page(
            "Setting up",
            None,
            vec![
                "In turn order and then back again, each player places a starting town with a road, then an outpost with a road.".to_owned(),
                format!("Everyone starts with {start}."),
            ],
        ),
        page(
            "Your turn",
            None,
            vec![
                "Roll the dice. Every building touching a tile with that number collects its resource: yours and everyone else's.".to_owned(),
                "Then build, buy knight cards, cast a spell, trade, deploy and command your knights, in any order.".to_owned(),
                "End your turn when you are done.".to_owned(),
            ],
        ),
    ];
    pages.extend(Item::ALL.into_iter().map(|item| Section {
        title: item.title().to_owned(),
        glyph: Some(item.glyph()),
        cost: Some(item.cost()),
        lines: std::iter::once(item.summary().to_owned()).chain(item.rules()).collect(),
    }));
    pages.extend(Spell::ALL.into_iter().map(|spell| Section {
        title: format!("Spell: {}", spell_title(spell)),
        glyph: Some(Glyph::Spell(spell)),
        cost: Some(spell.cost()),
        lines: std::iter::once(spell_summary(spell).to_owned()).chain(spell_rules(spell)).collect(),
    }));
    let (item_num, item_den) = balance::ITEM_SPAWN_CHANCE;
    let mut items = vec![
        format!(
            "At the end of each round, an item appears with a {}% chance on a free tile touching no town or castle.",
            item_num * 100 / item_den
        ),
        format!(
            "Nobody knows what it is until a knight walks onto its tile; knights can't deploy onto it. It vanishes after {} rounds, and a bandit landing on it takes it.",
            balance::ITEM_ROUNDS
        ),
    ];
    items.extend(ItemKind::ALL.into_iter().map(|kind| format!("• {}: {}", item_title(kind), item_rule(kind))));
    pages.push(page("Items", Some(Glyph::Chest), items));
    pages.extend([
        page(
            "Combat",
            None,
            vec![
                "Knight → bandit: the bandit and the knight both fall.".to_owned(),
                "Knight → knight: both knights fall.".to_owned(),
                "Every attack costs the attacking knight. One knight or bandit per tile.".to_owned(),
            ],
        ),
        page(
            "Bandits",
            Some(Glyph::Robber),
            vec![
                "A bandit blocks production on its tile. The island always has at least one: one stands from the start.".to_owned(),
                format!(
                    "Rolling {} produces nothing. The roller moves a bandit to any free tile, or places a new one when the island has none. It then stays put through the next full round.",
                    balance::ROBBER_ROLL
                ),
                "At the end of every round each bandit wanders to a free neighbouring tile.".to_owned(),
                format!(
                    "While there are fewer bandits than one per {} towns and castles on the island, another may appear at the end of a round ({}% chance).",
                    balance::SETTLEMENTS_PER_ROBBER,
                    spawn_num * 100 / spawn_den
                ),
                format!(
                    "A knight can bring a bandit down, falling with it. After that, no bandit appears for {} full rounds; then a new one appears if the island has none.",
                    balance::ROBBER_RESPAWN_ROUNDS
                ),
            ],
        ),
        page(
            "Trading",
            Some(Glyph::Trade),
            vec![
                format!(
                    "On your turn, trade {} of one resource for 1 of any other with the bank, as often as you like.",
                    balance::BANK_TRADE_RATE
                ),
                "Or offer resources to everyone or to one player. The first player to accept trades with you; anyone can decline.".to_owned(),
            ],
        ),
    ]);
    pages
}

/// What an attack will do, before it is made: a title and a sentence.
pub fn attack_outcome(g: &Game, names: &[String], unit: UnitId, target: Target) -> Option<(String, String)> {
    let name = |p: PlayerId| names.get(p.index()).cloned().unwrap_or_else(|| format!("Player {}", p.0 + 1));
    g.unit(unit)?;
    Some(match target {
        Target::Robber(_) => {
            ("Knight vs bandit".to_owned(), "The bandit is removed, and your knight falls with it.".to_owned())
        }
        Target::Unit(u) => {
            let defender = g.unit(u)?;
            ("Knight vs knight".to_owned(), format!("Both knights fall: yours and {}'s.", name(defender.owner)))
        }
    })
}
