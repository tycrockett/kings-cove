//! What every piece, spell and rule does, in words. Numbers come from the
//! engine's `balance` and outcomes from its own rules, so the text cannot
//! drift from the game.

use kings_cove_engine::{BuildingKind, Game, Lasting, PlayerId, Resources, Spell, Target, UnitId, balance};

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
            Item::City => "City",
            Item::Knight => "Knight",
        }
    }

    /// The button that buys it.
    pub fn verb(self) -> &'static str {
        match self {
            Item::Road => "Build a road",
            Item::Outpost => "Build an outpost",
            Item::Town => "Upgrade to a town",
            Item::City => "Upgrade to a city",
            Item::Knight => "Buy a knight card",
        }
    }

    /// Why there is nowhere to use it, when the player can pay but the board
    /// offers no place.
    pub fn no_place(self) -> &'static str {
        match self {
            Item::Road => "No free side touches your roads or buildings",
            Item::Outpost => "No free corner on your roads",
            Item::Town => "No outpost of yours has room: towns need no town or city next door",
            Item::City => "You have no town to upgrade",
            Item::Knight => "You cannot buy one now",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Item::Road => "Stretches your reach across the island.",
            Item::Outpost => "A new settlement at the end of your roads.",
            Item::Town => "An outpost grown into a town.",
            Item::City => "A town grown into a walled city.",
            Item::Knight => "A fighter that clears robbers and duels enemy knights.",
        }
    }

    pub fn rules(self) -> Vec<String> {
        match self {
            Item::Road => vec![
                "Sits on a side of a tile.".to_owned(),
                "Must touch one of your roads or buildings.".to_owned(),
                "New outposts can only go on a corner your roads reach.".to_owned(),
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
                "Upgrades one of your outposts in place, unless a town or city stands on a corner next to it.".to_owned(),
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
                "Your first city unlocks spells.".to_owned(),
            ],
            Item::Knight => vec![
                "Buying gives you a card. Deploy it from your hand onto a free tile touching your town or city (not an outpost).".to_owned(),
                "Each turn it either moves to a free neighbouring tile or attacks; it rests the turn it arrives.".to_owned(),
                "Attacks a neighbouring robber or enemy knight:".to_owned(),
                "• Robber: the robber and your knight both fall.".to_owned(),
                "• Knight: both knights fall.".to_owned(),
            ],
        }
    }
}

pub fn spell_title(spell: Spell) -> &'static str {
    match spell {
        Spell::Frost => "Frost",
        Spell::Bounty => "Bounty",
        Spell::Rockslide => "Rockslide",
        Spell::Motherlode => "Motherlode",
        Spell::Ward => "Ward",
        Spell::Lure => "Lure",
        Spell::Swap => "Swap",
        Spell::Rally => "Rally",
        Spell::Windfall => "Windfall",
    }
}

pub fn spell_summary(spell: Spell) -> &'static str {
    match spell {
        Spell::Frost => "Freezes a wood or food tile solid.",
        Spell::Bounty => "Blesses a wood or food tile with a double harvest.",
        Spell::Rockslide => "Buries a stone or iron tile under rubble.",
        Spell::Motherlode => "Opens a rich seam in a stone or iron tile.",
        Spell::Ward => "Raises a shield no robber can cross.",
        Spell::Lure => "Draws a robber wherever you want it.",
        Spell::Swap => "Two tiles trade their numbers.",
        Spell::Rally => "Your knights stand ready again.",
        Spell::Windfall => "Your next roll brings a bit of everything.",
    }
}

/// What the spell does, then the rules every spell shares.
pub fn spell_rules(spell: Spell) -> Vec<String> {
    let rolls = |effect: &str, tiles: &str| {
        format!(
            "Cast on a {tiles} tile. {effect} for the next {} times its number is rolled, for everyone with a building on it.",
            balance::SPELL_CHARGES
        )
    };
    let mut lines = vec![match spell {
        Spell::Frost => rolls("It produces nothing", "wood or food"),
        Spell::Bounty => rolls("It produces double", "wood or food"),
        Spell::Rockslide => rolls("It produces nothing", "stone or iron"),
        Spell::Motherlode => rolls("It produces double", "stone or iron"),
        Spell::Ward => format!(
            "Cast on any tile. For {} rounds no robber may enter or stay there: one standing on it is pushed to a neighbouring tile.",
            balance::WARD_ROUNDS
        ),
        Spell::Lure => {
            "Pick a robber, then any free tile: the robber moves there and stays put through the next full round."
                .to_owned()
        }
        Spell::Swap => format!(
            "Pick two tiles with different numbers: they trade numbers until either number has been rolled {} times between them.",
            balance::SPELL_CHARGES
        ),
        Spell::Rally => "Every one of your knights can move or attack again this turn.".to_owned(),
        Spell::Windfall => {
            "On your next roll, whatever comes up, you collect 1 of each resource your buildings touch.".to_owned()
        }
    }];
    if spell.lasting().is_some() {
        lines.push("A spell cast on a tile that already holds one replaces it.".to_owned());
    }
    lines.push(format!(
        "Spells unlock with your first city. Any spell empties your mana for {} rounds.",
        balance::MANA_RECHARGE_ROUNDS
    ));
    lines
}

/// What a lingering spell's counter counts.
pub fn charges_word(spell: Spell, n: u32) -> String {
    let word = match spell.lasting() {
        Some((Lasting::Rounds, _)) => "round",
        _ => "roll",
    };
    format!("{n} {word}{} left", if n == 1 { "" } else { "s" })
}

/// What the board asks while a spell is being aimed.
pub fn spell_hint(spell: Spell, picked_first: bool) -> String {
    match (spell, picked_first) {
        (Spell::Frost | Spell::Bounty, _) => format!("Tap a wood or food tile for {}", spell.name()),
        (Spell::Rockslide | Spell::Motherlode, _) => format!("Tap a stone or iron tile for {}", spell.name()),
        (Spell::Ward, _) => "Tap a tile to ward".to_owned(),
        (Spell::Lure, false) => "Tap the robber to lure".to_owned(),
        (Spell::Lure, true) => "Tap a free tile to lure it to".to_owned(),
        (Spell::Swap, false) => "Tap the first tile to swap".to_owned(),
        (Spell::Swap, true) => "Tap a tile with another number to swap with".to_owned(),
        (Spell::Rally | Spell::Windfall, _) => String::new(),
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
                        "The first player to the match's goal wins: {} victory points, chosen when it is created. Longer games play on a bigger island.",
                        goals_text()
                    ),
                },
                format!(
                    "Towns are worth {} VP and cities {}; outposts score nothing.",
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
    pages.extend([
        page(
            "Combat",
            None,
            vec![
                "Knight → robber: the robber and the knight both fall.".to_owned(),
                "Knight → knight: both knights fall.".to_owned(),
                "Every attack costs the attacking knight. One knight or robber per tile.".to_owned(),
            ],
        ),
        page(
            "Robbers",
            Some(Glyph::Robber),
            vec![
                "A robber blocks production on its tile. The island always has at least one: one stands from the start.".to_owned(),
                format!(
                    "Rolling {} produces nothing. The roller moves a robber to any free tile, or places a new one when the island has none. It then stays put through the next full round.",
                    balance::ROBBER_ROLL
                ),
                "At the end of every round each robber wanders to a free neighbouring tile.".to_owned(),
                format!(
                    "While there are fewer robbers than one per {} towns and cities on the island, another may appear at the end of a round ({}% chance).",
                    balance::SETTLEMENTS_PER_ROBBER,
                    spawn_num * 100 / spawn_den
                ),
                format!(
                    "A knight can bring a robber down, falling with it. After that, no robber appears for {} full rounds; then a new one appears if the island has none.",
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
            ("Knight vs robber".to_owned(), "The robber is removed, and your knight falls with it.".to_owned())
        }
        Target::Unit(u) => {
            let defender = g.unit(u)?;
            ("Knight vs knight".to_owned(), format!("Both knights fall: yours and {}'s.", name(defender.owner)))
        }
    })
}
