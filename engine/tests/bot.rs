use kings_cove_engine::{
    Action, Answer, Board, Event, Game, Phase, PlayerId, Resources, Rng, TileId, Unit, UnitId, UnitKind, balance,
    balance::{Amount, MapShape, MapSize, Settings},
    bot::{self, Style, Turn},
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn names(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("Bot {i}")).collect()
}

/// Plays `g` as the server does: the bot whose turn it is plays, and when it
/// waits on an offer the others answer it, for it to settle as it goes on.
/// Counts the events it saw.
fn play_out(g: &mut Game, styles: &[Style], rng: &mut Rng, seen: &mut Vec<Event>) {
    let mut turns = 0;
    let mut last_seq = 0;
    while g.winner().is_none() {
        let current = g.current;
        match bot::play_turn(g, current, styles[current.index()], rng).unwrap() {
            Turn::Over => turns += 1,
            Turn::Waiting => {
                for (i, &style) in styles.iter().enumerate() {
                    bot::answer_trade(g, PlayerId(i as u8), style, rng);
                }
                let players = styles.len();
                assert!(
                    g.trade.as_ref().is_none_or(|t| t.recipients(players).all(|p| t.answer(p).is_some())),
                    "every bot answers an offer made to it"
                );
            }
        }
        seen.extend(g.events.iter().filter(|e| e.seq > last_seq).map(|e| e.event.clone()));
        last_seq = g.event_seq;
        assert!(turns < 3000, "no winner after {turns} turns");
    }
}

/// Played to 10: buildings alone can't make 20 on the smallest island with
/// four players. Every shape and number of each structure comes up.
#[test]
fn bots_of_every_style_play_a_match_to_a_winner_with_trades_knights_spells_and_structures() {
    let mut seen = Vec::new();
    for (seed, size) in MapSize::ALL.into_iter().enumerate() {
        for players in balance::MIN_PLAYERS..=balance::MAX_PLAYERS {
            let mut rng = Rng::new(seed as u64 * 10 + players as u64);
            let settings = Settings {
                victory_points: 10,
                map_size: size,
                map_shape: MapShape::ALL[(seed + players) % MapShape::ALL.len()],
                shrines: Amount::ALL[(seed + players) % Amount::ALL.len()],
                mines: Amount::ALL[(seed + players + 1) % Amount::ALL.len()],
                mills: Amount::ALL[(seed + players + 2) % Amount::ALL.len()],
            };
            let mut g = Game::new(&names(players), &settings, &mut rng);
            let styles: Vec<Style> = (0..players).map(|i| Style::ALL[(i + seed) % Style::ALL.len()]).collect();
            play_out(&mut g, &styles, &mut rng, &mut seen);
            let winner = g.winner().unwrap();
            assert!(g.points(winner) >= g.victory_points);
        }
    }
    let count = |f: fn(&Event) -> bool| seen.iter().filter(|e| f(e)).count();
    assert!(count(|e| matches!(e, Event::Traded { .. })) > 0, "bots trade with each other");
    assert!(count(|e| matches!(e, Event::Deployed { .. })) > 0, "bots deploy knights");
    assert!(count(|e| matches!(e, Event::UnitMoved { .. })) > 0, "bots move knights");
    assert!(count(|e| matches!(e, Event::SpellCast { .. })) > 0, "bots cast spells");
    assert!(count(|e| matches!(e, Event::StructureTaken { .. })) > 0, "bots take structures");
    assert!(count(|e| matches!(e, Event::StructurePaid { .. })) > 0, "mines and mills pay bots");
}

/// Player 0's turn, rolled, a knight of each player's side by side on the
/// small island and nothing else on it.
fn knights_face_to_face() -> Game {
    let mut g = Game::with_board(&names(2), Board::hexagon(2));
    g.phase = Phase::Playing;
    g.round = 1;
    g.dice = Some((1, 2));
    for p in &mut g.players {
        p.resources = Resources::ZERO;
        p.cards.knights = 0;
    }
    let centre = g.board.tiles.iter().find(|t| t.q == 0 && t.r == 0).unwrap().id;
    let next: TileId = g.board.tile(centre).neighbors[0];
    for (id, owner, tile) in [(1, P0, centre), (2, P1, next)] {
        let unit = Unit { id: UnitId(id), owner, kind: UnitKind::Knight, tile, ready: true, relics: 0, blockade: None };
        g.units.push(unit);
    }
    g
}

#[test]
fn a_ruthless_bot_attacks_a_knight_a_friendly_one_leaves_alone() {
    for (style, attacks) in [(Style::Friendly, false), (Style::Ruthless, true)] {
        let mut g = knights_face_to_face();
        let mut rng = Rng::new(1);
        assert_eq!(bot::play_turn(&mut g, P0, style, &mut rng).unwrap(), Turn::Over);
        assert_eq!(g.unit(UnitId(2)).is_none(), attacks, "{style:?}");
        assert_eq!(g.current, P1, "{style:?} ends its turn");
    }
}

#[test]
fn a_bot_takes_a_trade_its_style_finds_worth_it_and_haggles_over_the_rest() {
    let mut rng = Rng::new(3);
    let mut g = after_setup(&mut rng);
    g.apply(P0, Action::Roll, &mut rng).unwrap();
    let wood = |n| balance::res(n, 0, 0, 0);
    let iron = |n| balance::res(0, 0, 0, n);
    // What the bot holds, the wood it is offered for some iron, and what a
    // friendly, balanced and ruthless bot answer: `None` takes it, `Some`
    // counters with those terms (from the maker's side).
    let cases = [
        (balance::res(1, 0, 0, 1), iron(1), [None, Some((wood(2), iron(1))), Some((wood(2), iron(1)))]),
        (iron(3), iron(3), [None, None, Some((wood(1), iron(2)))]),
        (iron(3), iron(1), [None, None, None]),
    ];
    for (holds, want, answers) in cases {
        for (style, answer) in Style::ALL.into_iter().zip(answers) {
            g.players[0].resources = wood(5);
            g.players[1].resources = holds;
            g.apply(P0, Action::ProposeTrade { to: Some(P1), give: wood(1), want }, &mut rng).unwrap();
            bot::answer_trade(&mut g, P1, style, &mut rng);
            let case = format!("{style:?} offered {want:?} for {holds:?}");
            match answer {
                None => assert_eq!((g.players[0].resources.wood, g.trade.as_ref()), (4, None), "{case}"),
                Some((give, want)) => {
                    let offer = g.trade.as_ref().expect("still open");
                    assert_eq!(offer.answer(P1), Some(Answer::Counter { give, want }), "{case}");
                    g.apply(P0, Action::ConfirmTrade { with: P1 }, &mut rng).unwrap();
                    assert_eq!(g.players[1].resources.wood, holds.wood + give.wood, "{case}: traded on the counter");
                }
            }
        }
    }
}

#[test]
fn a_bot_trades_on_a_counter_to_its_own_offer_it_finds_worth_it() {
    let mut rng = Rng::new(3);
    let mut g = after_setup(&mut rng);
    g.apply(P0, Action::Roll, &mut rng).unwrap();
    g.apply(P0, Action::EndTurn, &mut rng).unwrap();
    g.apply(P1, Action::Roll, &mut rng).unwrap();
    g.players[0].resources = balance::res(0, 0, 0, 3);
    g.players[1].resources = balance::res(4, 0, 0, 0);
    let (give, want) = (balance::res(1, 0, 0, 0), balance::res(0, 0, 0, 1));
    g.apply(P1, Action::ProposeTrade { to: Some(P0), give, want }, &mut rng).unwrap();
    let counter = Action::CounterTrade { give: balance::res(2, 0, 0, 0), want };
    g.apply(P0, counter, &mut rng).unwrap();
    bot::play_turn(&mut g, P1, Style::Balanced, &mut rng).unwrap();
    assert!(g.events.iter().any(|e| matches!(e.event, Event::Traded { from: P1, give, .. } if give.wood == 2)));
}

/// Two balanced bots' match on the smallest island, set up.
fn after_setup(rng: &mut Rng) -> Game {
    let settings = Settings { map_size: MapSize::ExtraSmall, ..Settings::default() };
    let mut g = Game::new(&names(2), &settings, rng);
    while matches!(g.phase, Phase::Setup { .. }) {
        let current = g.current;
        bot::play_turn(&mut g, current, Style::Balanced, rng).unwrap();
    }
    g
}
