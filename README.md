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
On a phone, open *Server settings* on the home screen and enter the server's LAN address (`http://192.168.x.y:8787`).
A client that closes or loses its connection resumes its seat on the next launch (the seat token is kept in the app's
storage); a restarted server resumes its matches from the data file.

In a match, **Menu** opens *How to play* (every rule and piece; also on the home screen), *Leave match* (this device
gives up its seat; the match goes on) and, for the host, *End match for everyone*. Tapping an item in the build tray
shows what it does and its price before buying it, and tapping an attack target shows the outcome before it is made.

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

- **Board**: 19 hex tiles of Wood, Stone, Food and Iron, numbered 2–12 without 7; the 6s and 8s never touch and no two
  neighbours share a number.
- **Buildings** sit on tile corners and are upgraded in place: **Outpost → Town → City**. Every new building starts as
  an outpost on the owner's roads.

  | Level | Victory points | Production per matching tile |
  |---|---|---|
  | Outpost | 0 | 1 |
  | Town | 1 | 1 |
  | City | 2 | 2 |

- **Roads** sit on tile sides and must connect to the owner's roads or buildings.
- **Setup**: in snake order each player places a starting town with a road, then an outpost with a road, and starts
  with a small stock of resources.
- **Turns**: roll two dice; every player's buildings on tiles showing the total collect, unless a robber stands there.
  Then trade, build, buy unit cards, deploy, and move or attack with units, and end the turn.
- **Units**: a Knight or Ram card deploys onto a free tile next to the owner's town or city (not an outpost). Units move
  one tile or attack once per turn, not both, and rest the turn they arrive. Knights destroy robbers and rams and trade
  themselves for enemy knights; rams knock an adjacent enemy building down a level (city → town → outpost → gone).
  One unit or robber per tile.
- **Robbers** block production on their tile. At the end of each round they wander to a free neighbouring tile; then,
  if there are fewer robbers than `floor(towns and cities / 4)`, one appears on a free tile with a 25% chance.
  Rolling 7 produces nothing and lets the roller move one robber anywhere free, or place a new one when the island has
  none; it then stays put through the next complete round.
- **Victory**: the first player to reach 10 victory points wins.

Costs (all in `engine/src/balance.rs`):

| Item | Cost |
|---|---|
| Road | 1 wood, 1 stone |
| Outpost | 2 wood, 1 stone |
| Town upgrade | 1 stone, 2 food |
| City upgrade | 3 stone, 2 food, 2 iron |
| Knight card | 1 food, 1 iron |
| Ram card | 2 wood, 2 iron |

## Playing on a phone

- Drag to pan, pinch to zoom (trackpad pinch, scroll and Ctrl+scroll on desktops).
- Tap a tray action (Road, Outpost, Town ↑, City ↑) or a unit card in your hand: legal spots glow; tap one to build.
- Tap one of your ready units (gold dot) to see gold moves and red attacks.
- Tap any tile, building or road to inspect it; tap the status line for the event history.

## Test

```sh
cargo test -p kings-cove-engine -p kings-cove-server
```
