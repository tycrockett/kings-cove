//! What every piece, spell and rule does, in words. Numbers come from the
//! engine's `balance` and outcomes from its own rules, so the text cannot
//! drift from the game.

use kings_cove_engine::{
    BuildingKind, Game, ItemKind, PlayerId, Resources, Spell, StructureKind, Target, TileId, UnitId, balance,
};

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
            Item::Knight => "Buy a knight token",
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
                "Close a ring of roads and buildings around a tile, one of them a castle, and it pays you every turn (see Rings).".to_owned(),
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
                "Once you have a castle, spells cost no resources (see Spells and mana).".to_owned(),
                "A ring with a castle on it pays you every turn (see Rings).".to_owned(),
            ],
            Item::Knight => vec![
                "Buying gives you a token. Deploy it onto a free tile touching your town or castle (not an outpost).".to_owned(),
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
        Spell::Ward => "Ward",
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
        Spell::Ward => "A deadly circle no bandit or rival knight survives.",
    }
}

/// What the spell does; the rules every spell shares are [`casting_rules`].
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
        Spell::Ward => format!(
            "Cast on any tile. Every bandit and every other player's knight on it or on a tile next to it falls at once. For {} rounds (this one and the next) any bandit or other player's knight that comes into it falls. Your own knights are safe there.",
            balance::WARD_ROUNDS
        ),
    }];
    if spell.lasting().is_some() || spell == Spell::Ward {
        lines.push("A spell cast on a tile that already holds one replaces it.".to_owned());
    }
    lines
}

/// How mana limits casting, in a sentence.
pub fn mana_rule() -> String {
    format!(
        "All your spells share one mana: casting any spell empties it, and it refills after {} rounds.",
        balance::MANA_RECHARGE_ROUNDS
    )
}

/// The rules every spell shares.
pub fn casting_rules() -> Vec<String> {
    vec![
        "Cast a spell from the spellbook on your turn, after you roll.".to_owned(),
        "Anyone can cast. Without a castle a spell costs the price on its page; once you have a castle, every spell is free.".to_owned(),
        mana_rule(),
        "A mana crystal refills your mana at once. A spell scroll casts its spell free, whatever your mana, and leaves your mana as it is.".to_owned(),
    ]
}

/// What a lingering spell's counter counts.
pub fn charges_word(n: u32) -> String {
    format!("{n} roll{} left", if n == 1 { "" } else { "s" })
}

/// What the board asks while a spell is being aimed.
pub fn spell_hint(spell: Spell, picked_first: bool) -> String {
    match (spell, picked_first) {
        (Spell::Frost | Spell::Bounty | Spell::Luck | Spell::Ward, _) => format!("Tap a tile for {}", spell.name()),
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
            "One free cast of a random spell: no price, whatever your mana, and your mana stays as it is.".to_owned()
        }
        ItemKind::Recruit => "A knight token.".to_owned(),
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
        ItemKind::Recruit => "Recruit: a knight token".to_owned(),
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
                        "The first player to the match's goal wins: {} victory points, chosen by the host in the lobby along with the island's size ({}), its shape (classic rings of tiles, or a wild coast of capes and bays) and how many shrines, mines and lumber mills stand on it. Or the host leaves it all to chance: random settings suit the number of players and stay secret until the match starts.",
                        goals_text(),
                        balance::MapSize::ALL
                            .iter()
                            .map(|s| format!("{} {} tiles", s.label().to_lowercase(), s.tiles()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                },
                format!(
                    "Towns are worth {} VP and castles {}; outposts score nothing. A relic counts {} VP for as long as one of your knights carries it, and a shrine {} VP for as long as one of your knights stands on it.",
                    balance::TOWN_POINTS,
                    balance::CITY_POINTS,
                    balance::RELIC_POINTS,
                    balance::SHRINE_POINTS
                ),
            ],
        ),
        page(
            "Setting up",
            None,
            vec![
                "In turn order and then back again, each player places a starting town with a road, then an outpost with a road.".to_owned(),
                format!(
                    "Everyone starts with {start} and {} knight token{}.",
                    balance::STARTING_KNIGHTS,
                    if balance::STARTING_KNIGHTS == 1 { "" } else { "s" }
                ),
            ],
        ),
        page(
            "Your turn",
            None,
            vec![
                "Roll the dice. Every building touching a tile with that number collects its resource: yours and everyone else's.".to_owned(),
                "Then build, buy knight tokens, cast a spell, trade, deploy and command your knights, in any order.".to_owned(),
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
    pages.push(page(
        "Rings",
        Some(Glyph::Road),
        vec![
            "Ring a tile: your roads on all six of its sides and your buildings on all six of its corners.".to_owned(),
            format!(
                "Once one of those buildings is a castle, the tile pays you {} of its resource at the start of each of your turns, whatever is rolled.",
                balance::RING_INCOME
            ),
            "A bandit or a blockade on the tile stops the payment while it stands there.".to_owned(),
            "Towns and castles can't stand on neighbouring corners, so at most three of a ring's buildings are towns or castles; the rest are outposts.".to_owned(),
        ],
    ));
    pages.push(page("Spells and mana", Some(Glyph::Spellbook), casting_rules()));
    pages.extend(Spell::ALL.into_iter().map(|spell| Section {
        title: format!("Spell: {}", spell_title(spell)),
        glyph: Some(Glyph::Spell(spell)),
        cost: Some(spell.cost()),
        lines: std::iter::once(spell_summary(spell).to_owned()).chain(spell_rules(spell)).collect(),
    }));
    let mut items = vec![
        format!(
            "Items are scattered over the island once it is set up, {} per player. At the end of every round at least one more appears, and more until there are {} per player again.",
            balance::ITEMS_PER_PLAYER,
            balance::ITEMS_PER_PLAYER
        ),
        "They turn up on free tiles, most often far from any knight.".to_owned(),
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
                "Instead of moving or attacking, a knight can blockade its tile: nothing is produced there until the blockade lifts (see Knight).".to_owned(),
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
                "At the end of every round each bandit wanders to a free neighbouring tile, unless a structure draws it (see Shrines, mines and mills).".to_owned(),
                format!(
                    "While there are fewer bandits than one per {} towns and castles, plus one per {} tiles of island, another may appear at the end of a round ({}% chance), as far from the others as it can.",
                    balance::SETTLEMENTS_PER_ROBBER,
                    balance::TILES_PER_EXTRA_ROBBER,
                    spawn_num * 100 / spawn_den
                ),
                format!(
                    "A knight can bring a bandit down, falling with it. After that, no bandit appears for {} full rounds; then a new one appears if the island has none.",
                    balance::ROBBER_RESPAWN_ROUNDS
                ),
            ],
        ),
        page(
            "Shrines, mines and mills",
            Some(Glyph::Shrine),
            {
                let mut lines = vec![
                    "When the host asks for them, shrines, mines and lumber mills stand on the island for the whole match, as far apart as they can be.".to_owned(),
                    "A structure serves the player whose knight stands on it, for as long as it stands there: moving on or falling ends it.".to_owned(),
                ];
                lines.extend(StructureKind::ALL.into_iter().map(|kind| format!("• {}: {}", structure_title(kind), structure_rule(kind))));
                lines.push(format!(
                    "At the end of every round each empty structure draws the bandit nearest to it, one bandit a structure: it moves {} toward it instead of wandering.",
                    if balance::STRUCTURE_BANDIT_STEPS == 1 {
                        "a tile".to_owned()
                    } else {
                        format!("{} tiles", balance::STRUCTURE_BANDIT_STEPS)
                    }
                ));
                lines.push("A bandit that reaches a structure holds it: it stays put, and no knight gains from it until one brings the bandit down (falling with it).".to_owned());
                lines
            },
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
        page(
            "Leaving a match",
            None,
            vec![
                "Closing the app or losing the connection keeps your seat: open the app again and you are back in the match.".to_owned(),
                "Menu → Leave match takes this device out of the match. Your seat stays, and the others wait on your turns.".to_owned(),
                "The host can end the match for everyone from the menu; every player goes back to the home screen.".to_owned(),
            ],
        ),
    ]);
    pages
}

/// A structure's name, capitalised.
pub fn structure_title(kind: StructureKind) -> &'static str {
    match kind {
        StructureKind::Shrine => "Shrine",
        StructureKind::Mine => "Mine",
        StructureKind::LumberMill => "Lumber mill",
    }
}

/// What sending `player`'s knight onto `tile` does when a rival's ward holds
/// it: a title, a sentence and the ward's caster.
pub fn ward_outcome(g: &Game, names: &[String], player: PlayerId, tile: TileId) -> Option<(String, String, PlayerId)> {
    let caster = g.wards_over(tile).find(|e| e.caster != player)?.caster;
    let name = names.get(caster.index()).cloned().unwrap_or_else(|| format!("Player {}", caster.0 + 1));
    Some(("Into a ward".to_owned(), format!("{name}'s ward fells your knight the moment it gets there."), caster))
}

/// What a structure gives the player whose knight stands on it, in a few words.
pub fn structure_gain(kind: StructureKind) -> String {
    match kind {
        StructureKind::Shrine => format!("+{} VP", balance::SHRINE_POINTS),
        StructureKind::Mine => format!("{} iron or stone each turn", balance::MINE_YIELD),
        StructureKind::LumberMill => format!("{} wood each turn", balance::MILL_YIELD),
    }
}

/// What a structure does, in a sentence.
pub fn structure_rule(kind: StructureKind) -> String {
    match kind {
        StructureKind::Shrine => {
            format!("worth {} VP to the player whose knight stands on it.", balance::SHRINE_POINTS)
        }
        StructureKind::Mine => format!(
            "pays the player whose knight stands on it {} iron or stone (drawn each time) at the start of each of their turns.",
            balance::MINE_YIELD
        ),
        StructureKind::LumberMill => format!(
            "pays the player whose knight stands on it {} wood at the start of each of their turns.",
            balance::MILL_YIELD
        ),
    }
}

/// What an attack will do, before it is made: a title and a sentence.
pub fn attack_outcome(g: &Game, names: &[String], unit: UnitId, target: Target) -> Option<(String, String)> {
    let name = |p: PlayerId| names.get(p.index()).cloned().unwrap_or_else(|| format!("Player {}", p.0 + 1));
    let attacker = g.unit(unit)?;
    let mut outcome = match target {
        Target::Robber(r) => {
            let mut text = "The bandit is removed, and your knight falls with it.".to_owned();
            if let Some(kind) = g.robber(r).and_then(|r| g.structure_at(r.tile)) {
                text += &format!(" The {} stands empty again.", kind.name());
            }
            ("Knight vs bandit".to_owned(), text)
        }
        Target::Unit(u) => {
            let defender = g.unit(u)?;
            let mut text = format!("Both knights fall: yours and {}'s.", name(defender.owner));
            if let Some(kind) = g.structure_at(defender.tile) {
                text += &format!(" {} loses the {} ({}).", name(defender.owner), kind.name(), structure_gain(kind));
            }
            ("Knight vs knight".to_owned(), text)
        }
    };
    if let Some(kind) = g.structure_at(attacker.tile) {
        outcome.1 += &format!(" You lose your {} ({}).", kind.name(), structure_gain(kind));
    }
    Some(outcome)
}
