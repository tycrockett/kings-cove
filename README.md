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

Open the client on 2–4 devices. One player taps **Create match** and shares the room code; the others **Join** with it.
In the room, the host picks the goal and island size; everyone sees the choice. The home screen shows the client's
version and the commit it was built from.
On a phone, open *Server settings* on the home screen and enter the server's LAN address (`http://192.168.x.y:8787`).
A client that closes or loses its connection resumes its seat on the next launch (the seat token is kept in the app's
storage); a restarted server resumes its matches from the data file.

In a match, **Menu** opens *How to play* (every rule and piece; also on the home screen), *Leave match* (this device
gives up its seat; the match goes on) and, for the host, *End match for everyone*. Tapping an item in the build tray
shows what it does and its price before buying it, and tapping an attack target shows the outcome before it is made.

To try the client alone, run a debug server and a debug client (`cargo run` builds both that way) and tap **Practice
alone (dev)** on the home screen. It starts a match against *Rival*, whom the server plays: it declines trades, and on
its turns it only rolls and passes, so play comes straight back to you. The match starts on your first turn. Setup is
already done, your first town is a castle, you have 12 of each resource and 2 knight tokens, and each side has two
knights on the board, one of Rival's next to one of yours. Release builds have neither the button nor the server's
`/practice` endpoint.

Server settings: `KC_ADDR` (listen address), `KC_DATA` (data file). Client build setting: `KC_SERVER_URL`, the server a
fresh install talks to (default `http://127.0.0.1:8787`). On a desktop, `KC_DATA_DIR=<dir>` gives a window its own
storage, so several local windows each take their own seat.

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
  small, small, medium, large or extra large island of 19, 37, 61, 91 or 127 tiles (default medium), both chosen by the
  host in the room before the match starts.
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
  stands there. Then trade (with players, or 4 of a kind for 1 of anything with the bank), build, buy knight tokens,
  cast a spell, deploy, and move, attack or blockade with knights, and end the turn.
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
  of coming up as a chosen tile's number. Tile spells affect everyone on the tile, and a new one on a tile replaces the
  old.
- **Bandits** block production on their tile. The island always has at least one, from the start. At the end of each
  round they wander to a free neighbouring tile; then, while there are fewer than
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
- **Victory**: the first player to reach the match's goal (relics included) wins.

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

## Playing on a phone

- Drag to pan, pinch to zoom (trackpad pinch, scroll and Ctrl+scroll on desktops).
- Tap a tray action (Road, Outpost, Town ↑, Castle ↑) or a unit token in your hand: legal spots glow; tap one to build.
- Tap one of your ready units (gold dot) to see gold moves and red attacks.
- Tap any tile, building or road to inspect it. The dark strip over the dock shows what to do, or the latest action,
  and your points; tap it for the event history.
- Tapping a tile or a unit zooms in on it and the two rings of tiles around it; **Recenter**, at the left over the dark
  strip, zooms back out, as does tapping that tile again or the sea. Building and moving leave the view where it is.
- The chip at the top left is the player whose turn it is; tap it to see everyone's points, resources, knight tokens
  and mana.
- The smiley left of the main button sends a reaction everyone sees in the middle of the screen.

## Test

```sh
cargo test -p kings-cove-engine -p kings-cove-server
```
