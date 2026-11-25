//! What every piece and rule does, in words. Numbers come from the engine's
//! `balance` and outcomes from its own rules, so the text cannot drift from
//! the game.

use kings_cove_engine::{BuildingKind, Game, PlayerId, Resources, Target, UnitId, UnitKind, balance};

use crate::art::Glyph;

/// Something the build tray offers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Road,
    Outpost,
    Town,
    City,
    Knight,
    Ram,
}

impl Item {
    pub const ALL: [Item; 6] = [Item::Road, Item::Outpost, Item::Town, Item::City, Item::Knight, Item::Ram];

    pub fn cost(self) -> Resources {
        match self {
            Item::Road => balance::ROAD_COST,
            Item::Outpost => balance::OUTPOST_COST,
            Item::Town => balance::TOWN_UPGRADE_COST,
            Item::City => balance::CITY_UPGRADE_COST,
            Item::Knight => balance::KNIGHT_CARD_COST,
            Item::Ram => balance::RAM_CARD_COST,
        }
    }

    pub fn glyph(self) -> Glyph {
        match self {
            Item::Road => Glyph::Road,
            Item::Outpost => Glyph::Building(BuildingKind::Outpost),
            Item::Town => Glyph::Building(BuildingKind::Town),
            Item::City => Glyph::Building(BuildingKind::City),
            Item::Knight => Glyph::Unit(UnitKind::Knight),
            Item::Ram => Glyph::Unit(UnitKind::Ram),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Item::Road => "Road",
            Item::Outpost => "Outpost",
            Item::Town => "Town",
            Item::City => "City",
            Item::Knight => "Knight",
            Item::Ram => "Ram",
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
            Item::Ram => "Buy a ram card",
        }
    }

    /// Why there is nowhere to use it, when the player can pay but the board
    /// offers no place.
    pub fn no_place(self) -> &'static str {
        match self {
            Item::Road => "No free side touches your roads or buildings",
            Item::Outpost => "No free corner on your roads",
            Item::Town => "You have no outpost to upgrade",
            Item::City => "You have no town to upgrade",
            Item::Knight | Item::Ram => "You cannot buy one now",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Item::Road => "Stretches your reach across the island.",
            Item::Outpost => "A new settlement at the end of your roads.",
            Item::Town => "An outpost grown into a town.",
            Item::City => "A town grown into a walled city.",
            Item::Knight => "A fighter that clears robbers and defends against rams.",
            Item::Ram => "A siege engine that knocks enemy buildings down.",
        }
    }

    pub fn rules(self) -> Vec<String> {
        let unit_moves =
            "Each turn it either moves to a free neighbouring tile or attacks; it rests the turn it arrives.";
        let deploys = "Buying gives you a card. Deploy it from your hand onto a free tile touching your town or city (not an outpost).";
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
                "Upgrade it to a town to score and to deploy units beside it.".to_owned(),
                "A ram hitting it razes it.".to_owned(),
            ],
            Item::Town => vec![
                "Upgrades one of your outposts in place.".to_owned(),
                format!(
                    "Worth {} VP. Collects {} from each neighbouring tile whose number is rolled.",
                    balance::TOWN_POINTS,
                    balance::TOWN_YIELD
                ),
                "Knights and rams deploy onto the tiles it touches.".to_owned(),
                "A ram hitting it knocks it back to an outpost.".to_owned(),
            ],
            Item::City => vec![
                "Upgrades one of your towns in place.".to_owned(),
                format!(
                    "Worth {} VP. Collects {} from each neighbouring tile whose number is rolled.",
                    balance::CITY_POINTS,
                    balance::CITY_YIELD
                ),
                "A ram hitting it knocks it back to a town.".to_owned(),
            ],
            Item::Knight => vec![
                deploys.to_owned(),
                unit_moves.to_owned(),
                "Attacks a neighbouring robber, enemy knight or enemy ram:".to_owned(),
                "• Robber: the robber is removed.".to_owned(),
                "• Knight: both knights fall.".to_owned(),
                "• Ram: the ram is destroyed and your knight survives.".to_owned(),
            ],
            Item::Ram => vec![
                deploys.to_owned(),
                unit_moves.to_owned(),
                "Attacks an enemy building on a corner of its own tile, one level at a time: city → town → outpost → gone. The owner loses the points.".to_owned(),
                "Cannot fight units or robbers. An enemy knight next to it can destroy it.".to_owned(),
            ],
        }
    }
}

/// A page of the rulebook.
pub struct Section {
    pub title: String,
    pub glyph: Option<Glyph>,
    pub cost: Option<Resources>,
    pub lines: Vec<String>,
}

pub fn rulebook() -> Vec<Section> {
    let page = |title: &str, glyph, lines: Vec<String>| Section { title: title.to_owned(), glyph, cost: None, lines };
    let (spawn_num, spawn_den) = balance::ROBBER_SPAWN_CHANCE;
    let start = balance::STARTING_RESOURCES;
    let mut pages = vec![
        page(
            "Goal",
            Some(Glyph::Building(BuildingKind::City)),
            vec![
                format!("The first player to {} victory points wins.", balance::VICTORY_POINTS_TO_WIN),
                format!("Towns are worth {} VP and cities {}; outposts score nothing.", balance::TOWN_POINTS, balance::CITY_POINTS),
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
                "Then build, buy unit cards, trade, deploy and command your units, in any order.".to_owned(),
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
    pages.extend([
        page(
            "Combat",
            None,
            vec![
                "Knight → robber: the robber is removed.".to_owned(),
                "Knight → knight: both knights fall.".to_owned(),
                "Knight → ram: the ram is destroyed.".to_owned(),
                "Ram → building: city → town → outpost → gone.".to_owned(),
                "A unit that attacks is done until your next turn. One unit or robber per tile.".to_owned(),
            ],
        ),
        page(
            "Robbers",
            Some(Glyph::Robber),
            vec![
                "A robber blocks production on its tile.".to_owned(),
                format!(
                    "Rolling {} produces nothing. The roller moves a robber to any free tile, or places a new one when the island has none. It then stays put through the next full round.",
                    balance::ROBBER_ROLL
                ),
                "At the end of every round each robber wanders to a free neighbouring tile.".to_owned(),
                format!(
                    "Then, while there are fewer robbers than one per {} towns and cities on the island, a new one may appear ({}% chance).",
                    balance::SETTLEMENTS_PER_ROBBER,
                    spawn_num * 100 / spawn_den
                ),
                "Knights remove robbers.".to_owned(),
            ],
        ),
        page(
            "Trading",
            Some(Glyph::Trade),
            vec![
                "On your turn, offer resources to everyone or to one player.".to_owned(),
                "The first player to accept trades with you; anyone can decline.".to_owned(),
            ],
        ),
    ]);
    pages
}

/// What an attack will do, before it is made: a title and a sentence.
pub fn attack_outcome(g: &Game, names: &[String], unit: UnitId, target: Target) -> Option<(String, String)> {
    let name = |p: PlayerId| names.get(p.index()).cloned().unwrap_or_else(|| format!("Player {}", p.0 + 1));
    let attacker = g.unit(unit)?;
    let rests = "Your unit is then done until your next turn.";
    Some(match (attacker.kind, target) {
        (UnitKind::Knight, Target::Robber(_)) => {
            ("Knight vs robber".to_owned(), format!("The robber is removed. {rests}"))
        }
        (UnitKind::Knight, Target::Unit(u)) => {
            let defender = g.unit(u)?;
            match defender.kind {
                UnitKind::Knight => {
                    ("Knight vs knight".to_owned(), format!("Both knights fall: yours and {}'s.", name(defender.owner)))
                }
                UnitKind::Ram => {
                    ("Knight vs ram".to_owned(), format!("{}'s ram is destroyed. {rests}", name(defender.owner)))
                }
            }
        }
        (UnitKind::Ram, Target::Building(v)) => {
            let building = g.building(v)?;
            let owner = name(building.owner);
            let was = building.kind;
            let text = match was.downgrade() {
                Some(now) => format!(
                    "{owner}'s {} becomes {} {}, losing {} VP. {rests}",
                    was.name(),
                    if now == BuildingKind::Outpost { "an" } else { "a" },
                    now.name(),
                    was.points() - now.points()
                ),
                None => format!("{owner}'s outpost is razed. {rests}"),
            };
            (format!("Ram vs {}", was.name()), text)
        }
        _ => return None,
    })
}
