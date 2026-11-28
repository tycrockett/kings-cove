# King's Cove

A small online multiplayer, turn-based strategy game in the spirit of Settlers of Catan, written entirely in Rust.
The client is a native [Native Shell](https://nsh.dev) app (AppKit, UIKit, Android views, GTK, Win32), drawn with
NSH's canvas and touch handling — no HTML, JavaScript or web view.

| Crate | Role |
|---|---|
| `engine` | The rules: board topology, a deterministic state machine (`Game::apply`), seeded randomness, every balancing constant (`engine/src/balance.rs`). No UI, no I/O. |
| `protocol` | JSON messages between client and server. |
| `server` | The authoritative server: lobbies, room codes, seat tokens, long-polling, persistence. |
| `app` | The NSH client: board canvas (pinch, pan, tap), HUD, lobby. |

## Run

```sh
# The server (listens on 0.0.0.0:8787; matches are saved to kings-cove-data.json)
cargo run -p kings-cove-server

# A client on this desktop (phone-sized window)
cargo run -p kings-cove-app
# or through NSH, on any target
cd app && nsh run                     # --target ios | android | macos | linux | windows
```

Open the client on 2–4 devices. One player taps **Create match** and shares the room: **Share room link** sends a link
that opens the web client with the code filled in (through the system's share sheet: a message, mail, another app; or
copied, where there is none), and the code itself copies with a tap. The others **Join** with it.
In the room, the host picks the goal, the island's size and shape and how many shrines, mines and lumber mills stand on
it, or **Random**: the server then draws
them as the match starts, to suit the number of players, and nobody sees them before. Everyone sees the choice. Under
*Add a bot* the host can also seat bots the server plays, alone or alongside other players, each of a style:

| Style | Name | Trades | Works against |
|---|---|---|---|
| Friendly | `Friendly Bot 1` | takes offers worth at least 80% of what it gives | only a player a point from winning |
| Balanced | `Bot 1` | takes offers it gains from | whoever leads it or is 3 points from winning |
| Ruthless | `Ruthless Bot 1` | takes offers worth 150% of what it gives, only from players not ahead of it | everyone, leaders most |

An offer a bot won't take as it stands, from a player it trades with, it counters with the smallest change it would
take (one fewer of what is asked, or one more of what is given, or two such steps), and it takes a counter to its own
offer on the same terms.

A bot takes its starting corners by how often their numbers come up. On its turns it rolls, sends a bandit where it
hurts those it works against after a 7 (or just off its own tiles), sends its knights for items, onto empty shrines,
mines and lumber mills (where they stay) and against bandits on its tiles or on those structures, attacks knights and
blockades tiles of those it works against, casts a ward where it fells bandits or rivals' knights, builds a castle, a
town, an outpost or a
road, offers a player one of what it has most for one of what it lacks (once a turn), trades with the bank at the
market's rates, keeps
one knight (a ruthless bot two) and casts a spell when a castle or a scroll makes it free, or when it is worth its
price. Its turns happen as soon as the turn passes to it; an offer it makes to a person waits up to 30 seconds for an
answer, then the bot withdraws it and plays on. A bot never reads what a hidden item is. The home screen shows the
client's version and the commit it was built from.
On a phone, open *Settings* on the home screen and enter the server's LAN address (`http://192.168.x.y:8787`).
A client that closes or loses its connection resumes its seat on the next launch (the seat token is kept in the app's
storage); a restarted server resumes its matches from the data file.

In a match, **Menu** opens *How to play* (every rule and piece; also on the home screen), *Leave match* (this device
gives up its seat; the match goes on) and, for the host, *End match for everyone*. Tapping an item in the build tray
shows what it does and its price before buying it, and tapping an attack target shows the outcome before it is made.

To try the client alone, run a debug server and a debug client (`cargo run` builds both that way) and tap **Practice
alone (dev)** on the home screen. It starts a match against *Rival*, whom the server plays: it declines trades, and on
its turns it only rolls and passes, so play comes straight back to you. The match starts on your first turn. Setup is
already done, your first town is a castle, you have 12 of each resource and 2 knight tokens, each side has two
knights on the board, one of Rival's next to one of yours, and the island has a shrine, a mine and a lumber mill or
two. Release builds have
neither the button nor the server's
`/practice` endpoint.

Server settings: `KC_ADDR` (listen address), `KC_DATA` (data file). Client build settings: `KC_SERVER_URL`, the server a
fresh install talks to (default `http://127.0.0.1:8787`), and `KC_WEB_URL`, where room links from native builds point
(default `https://tycrockett.github.io/kings-cove/`; the web client links to its own address). On a desktop,
`KC_DATA_DIR=<dir>` gives a window its own storage, so several local windows each take their own seat.

## Deploy

The server runs on [Fly.io](https://fly.io) and the web client on GitHub Pages.

```sh
# Server: one machine, matches kept on a volume (fly.toml, Dockerfile)
fly apps create kings-cove
fly volumes create kings_cove_data --region iad --size 1 -a kings-cove
fly deploy

# Web client: .github/workflows/pages.yml builds `nsh package --target web` on every push to main.
# It points the client at the repository variable KC_SERVER_URL:
gh variable set KC_SERVER_URL --body https://kings-cove.fly.dev
```

Building the web client locally needs `rustup target add wasm32-unknown-unknown` and an LLVM toolchain
(`CC_wasm32_unknown_unknown=clang AR_wasm32_unknown_unknown=llvm-ar`; on macOS Homebrew's `llvm`), then
`cd app && nsh package --target web` writes the static site to `.nsh/out/`.

## Rules

- **Board**: hex tiles of Wood, Stone, Food and Iron, numbered 2–12 without 7; the 6s and 8s never touch and no two
  neighbours share a number. A match is played to 5, 10, 15, 20, 30, 40 or 50 victory points (default 20) on an extra
  small, small, medium, large or extra large island of 19, 37, 61, 91 or 127 tiles (default medium), shaped *classic*
  (rings of tiles around a centre tile) or *wild* (a free-form coast of capes and bays, in one piece and without lakes),
  all chosen by the host in the room before the match starts. *Random* settings draw an island for the players (extra
  small or small for 2, small or medium for 3, medium to extra large for 4), a goal for the island (10; 10–15; 15–20;
  20–30; 30–40 from extra small up), and any shape and number of each structure.
- **Structures**: shrines, mines and lumber mills (each off, few or many: 1, 1, 2, 3, 4 or 2, 2, 4, 6, 8 on the five
  sizes) stand for the whole match, all as far apart as they can be, each on a tile of its own with no resource and no
  number, which never produces. One serves the player whose knight stands
  on it, for as long as it stands there: a *shrine* is worth 2 VP; a *mine* pays 1 iron or 1 stone (drawn) and a
  *lumber mill* 1 wood at the start of each of that player's turns. At the end of each round every empty structure
  draws the nearest bandit not yet drawn one tile toward it; a bandit on a structure holds it, staying put until a
  knight brings it down.
- **Buildings** sit on tile corners and are upgraded in place: **Outpost → Town → Castle**. Every new building starts as
  an outpost on the owner's roads. Towns and castles never stand on neighbouring corners: an outpost next to a town or
  castle can't be upgraded, and starting towns keep the same distance.

  | Level | Victory points | Production per matching tile |
  |---|---|---|
  | Outpost | 0 | 1 |
  | Town | 1 | 1 |
  | Castle | 2 | 2 |

- **Roads** sit on tile sides and must connect to the owner's roads or buildings. A player whose roads cover all six
  sides of a tile and whose buildings stand on all six of its corners, one of them a castle, has *ringed* it: it pays
  them 1 of its resource at the start of each of their turns, unless a bandit or a blockade stands on it.
- **Setup**: in snake order each player places a starting town with a road, then an outpost with a road, and starts
  with a small stock of resources and one knight token.
- **Turns**: roll two dice; every player's buildings on tiles showing the total collect, unless a bandit or a blockade
  stands there. Then trade, build, buy knight tokens, cast a spell, deploy, and move, attack or blockade with knights,
  and end the turn.
- **Trading**: with the bank, 4 of a resource for 1 of any other, except this round's *market*: each round one
  resource drawn at random sells 2 for 1 and another 3 for 1. With players, the active player offers resources to
  everyone or to one player; each answers *Accept*, *Counter* (other terms) or *Decline* (final). When only one player
  can take the offer, their accepting trades at once; otherwise the maker picks whom to trade with among those who
  accepted or countered, on their terms. One offer stands at a time; withdrawing it and offering again revises it.
- **Knights**: a knight token deploys onto a free tile next to the owner's town or castle (not an outpost). Knights move
  one tile or attack once per turn, not both, and rest the turn they arrive. A knight that attacks a bandit or an enemy
  knight falls with it. One knight or bandit per tile. Instead of acting, a knight can *blockade* its tile: the tile
  produces nothing for anyone and the knight can neither move nor attack. Lifting the blockade is an order too; the
  blockade then holds for 2 more full rounds before the knight is free.
- **Spells** are cast from the spellbook on a player's turn, by anyone: they cost their price in resources, or nothing
  once the caster has a castle. Any spell empties the caster's mana for 4 rounds. *Frost* stops any tile producing for
  the next 2 rolls of its number; *Bounty* doubles it.
  *Lure* moves any bandit to any free tile. *Swap* trades two tiles' numbers for 2 rolls. *Windfall* pays 1 of each
  resource the caster's buildings touch on their next roll. *Luck* gives the next roll, whoever makes it, a 50% chance
  of coming up as a chosen tile's number. *Ward* fells every bandit and every other player's knight on its tile and the
  six around it; for that round and the next, any bandit or other player's knight coming into them falls (the client
  asks before you send a knight in). Tile spells affect everyone on the tile, and a new one on a tile replaces the old.
- **Bandits** block production on their tile. The island always has at least one, from the start. At the end of each
  round they wander to a free neighbouring tile (unless a structure draws them); then, while there are fewer than
  `floor(towns and castles / 4) + floor(tiles / 40)` (so 0, 0, 1, 2 or 3 more on the five island sizes), another
  appears with a 50% chance on a free tile, more likely the farther it is from the other bandits. After a knight removes
  a bandit none appear for two full rounds; then one appears if the island has none. Rolling 7 produces nothing and lets
  the roller move one bandit anywhere free, or place a new one when the island has none; it then stays put through the
  next complete round.
- **Items**: when setup ends, 2 items per player appear; at the end of every round at least one more appears, and more
  until there are 2 per player again (found and dropped ones don't count). They land on free tiles, more often the
  farther they are from any knight, but never on the last tile a player holding a knight token could deploy onto. An
  item stays hidden until a knight walks onto its tile (knights can't deploy onto
  it), vanishes after 4 rounds, and is taken by any bandit landing on it. *Iron ore* gives 2 iron; *supplies* 2 of one
  random resource; a *mana crystal* refills the finder's mana; a *spell scroll* one free cast of a random spell (no
  price, castle or mana needed); a *recruit* a knight token; a *relic* is carried by the knight for 1 VP and dropped
  where it falls; a *trap* fells the knight; a *bandit's hoard* gives 2 of each resource but brings a bandit next to it.
- **Victory**: the first player to reach the match's goal (relics and shrines included) wins.

Costs (all in `engine/src/balance.rs`):

| Item | Cost |
|---|---|
| Road | 1 wood, 1 stone |
| Outpost | 2 wood, 1 stone |
| Town upgrade | 1 stone, 2 food, 1 iron |
| Castle upgrade | 3 stone, 2 food, 2 iron |
| Knight token | 1 food, 1 iron |
| Frost spell | 1 stone, 3 iron |
| Bounty spell | 1 wood, 1 food, 3 iron |
| Lure spell | 1 food, 3 iron |
| Swap spell | 1 wood, 1 stone, 1 food, 3 iron |
| Windfall spell | 1 wood, 1 food, 1 iron |
| Luck spell | 1 wood, 3 iron |
| Ward spell | 2 stone, 1 food, 3 iron |

## Playing on a phone

- Drag to pan, pinch to zoom (trackpad pinch, scroll and Ctrl+scroll on desktops).
- Tap a tray action (Road, Outpost, Town ↑, Castle ↑) or a unit token in your hand: legal spots glow; tap one to build.
- The trade button right of your resources opens trading: tap a resource you can sell at the market's rate, then what
  you want for it; or tap resources into *You give* and *You get* (− takes one back) and offer them. A card you can't
  afford yet shows what you're short, with *Ask the table* (an offer filled in for you) and, when the bank can cover
  it, a one-tap bank trade. An offer everyone declined comes back to sweeten and send again.
- Tap one of your ready units (gold dot) to see gold moves and red attacks.
- Tap any tile, building or road to inspect it. The dark strip over the dock shows what to do, or the latest action,
  and your points; tap it for the event history.
- Tapping a tile or a unit zooms in on it and the two rings of tiles around it; **Recenter**, at the left over the dark
  strip, zooms back out, as do tapping that tile again or the sea and ending the turn. Building and moving leave the
  view where it is.
- The chip at the top left is the player whose turn it is; tap it to see everyone's points, resources, knight tokens
  and mana.
- The smiley left of the main button sends a reaction everyone sees in the middle of the screen.
- Sound: effects go with the dice, harvests, turns, building, trades, fights, spells and finds, and a Norse tune of
  lyre, drums and drone loops while a match is on screen. *Sound effects* and *Music* turn each off, in **Menu** or
  in *Settings* on the home screen. Every sound is synthesized when the app starts (`app/src/synth.rs`); no audio
  files ship with it. In the web client, sound starts with the first tap, as browsers require.

## Test

```sh
cargo test -p kings-cove-engine -p kings-cove-server
```
