use crownline::{
    board::{Position, Side, resolve_turn},
    game::{ClockSpec, Command, Game, History, Policy, Reason},
};

const CYCLE: [&str; 4] = ["1-5", "32-28", "5-1", "28-32"];

#[test]
fn repetition_claim_is_prospective_and_history_sensitive() {
    let initial = "B:BK1:WK32".parse().unwrap();
    let mut game = Game::new(initial, Policy::Tournament, None, true, 0).unwrap();
    for text in CYCLE.into_iter().cycle().take(7) {
        let side = game.position().side();
        game.apply(side, Command::Move { turn: text.into() }, 0, 0)
            .unwrap();
    }
    let side = game.position().side();
    let turn = resolve_turn(game.position(), "28-32").unwrap();
    assert!(
        !History::new(game.position())
            .prospective_repetition(turn)
            .unwrap()
    );
    assert!(game.history().prospective_repetition(turn).unwrap());
    let previous = game.position();
    game.apply(
        side,
        Command::ClaimRepetition {
            turn: turn.to_string(),
        },
        0,
        0,
    )
    .unwrap();
    assert_eq!(game.position(), previous);
    assert_eq!(game.outcome().unwrap().reason, Reason::Repetition);
}

#[test]
fn no_progress_requires_both_players_forty_turns() {
    let initial = "B:BK1:WK32".parse().unwrap();
    let mut game = Game::new(initial, Policy::Tournament, None, true, 0).unwrap();
    for (ply, text) in CYCLE.into_iter().cycle().take(80).enumerate() {
        let side = game.position().side();
        assert!(
            game.apply(side, Command::ClaimNoProgress, ply as u64, 0)
                .is_err()
        );
        game.apply(side, Command::Move { turn: text.into() }, ply as u64, 0)
            .unwrap();
    }
    assert_eq!(game.position(), initial);
    game.apply(Side::White, Command::ClaimNoProgress, 80, 0)
        .unwrap();
    assert_eq!(game.outcome().unwrap().reason, Reason::NoProgress);
}

#[test]
fn monotonic_clock_increment_pause_and_flag() {
    let spec = ClockSpec {
        initial_ms: 100,
        increment_ms: 10,
    };
    let mut game = Game::new(Position::start(), Policy::House, Some(spec), true, 5).unwrap();
    game.apply(Side::Black, Command::Pause, 35, 1000).unwrap();
    game.tick(150, 990).unwrap();
    assert_eq!(game.clock().unwrap().black_ms, 70);
    game.apply(Side::Black, Command::Resume, 151, 1010).unwrap();
    game.apply(
        Side::Black,
        Command::Move {
            turn: "9-13".into(),
        },
        170,
        1020,
    )
    .unwrap();
    assert_eq!(game.clock().unwrap().black_ms, 61);
    assert!(game.tick(169, 1030).is_err());
    assert!(
        game.apply(
            Side::White,
            Command::Move {
                turn: "21-17".into()
            },
            270,
            1040
        )
        .is_err()
    );
    assert_eq!(game.outcome().unwrap().winner, Some(Side::Black));
    assert_eq!(game.outcome().unwrap().reason, Reason::Flag);
    assert_eq!(game.clock().unwrap().white_ms, 0);
}
