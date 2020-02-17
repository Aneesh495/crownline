use crownline::{
    board::Position,
    eval::{Classical, Evaluator, features},
    game::History,
    moves,
    search::{Config, Limits, Rules, analyze, minimax},
};
use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

#[test]
fn optimized_search_matches_independent_minimax() {
    let mut position = Position::start();
    for i in 0..60 {
        let history = History::new(position);
        for depth in 1..=3 {
            let expected = minimax(&history, &Classical, depth, Rules::House);
            for size in [0, 64, 32768] {
                let result = analyze(
                    &history,
                    &Classical,
                    Limits::fixed(depth, 2_000_000),
                    Config {
                        table_entries: size,
                        ..Config::default()
                    },
                );
                assert_eq!(result.abandoned, None);
                assert_eq!(
                    result.score,
                    Some(expected),
                    "{position} depth {depth} table {size}"
                );
                let mut replay = position;
                for turn in result.pv {
                    replay = replay.play(turn).unwrap();
                }
            }
        }
        let legal = moves::legal(position);
        position = if legal.is_empty() || i % 20 == 19 {
            Position::start()
        } else {
            position.play(legal[(i * 7 + 3) % legal.len()]).unwrap()
        };
    }
}

#[test]
fn fixed_node_runs_and_fallback_are_deterministic() {
    let history = History::new(Position::start());
    let a = analyze(
        &history,
        &Classical,
        Limits::fixed(25, 16_000),
        Config::default(),
    );
    let b = analyze(
        &history,
        &Classical,
        Limits::fixed(25, 16_000),
        Config::default(),
    );
    assert_eq!(a.nodes, 16_000);
    assert_eq!(a.best, b.best);
    assert_eq!(a.pv, b.pv);
    assert_eq!(a.score, b.score);
    assert_eq!(a.cutoffs, b.cutoffs);
    let result = analyze(
        &history,
        &Classical,
        Limits::fixed(25, 0),
        Config::default(),
    );
    assert!(moves::legal(history.current()).contains(&result.best.unwrap()));
    assert_eq!(result.score, None);
    assert_eq!(result.nodes, 0);
}

#[test]
fn cancellation_and_expired_deadlines_preserve_input() {
    let history = History::new(Position::start());
    let cancel = AtomicBool::new(true);
    let mut limits = Limits::fixed(25, u64::MAX);
    limits.cancel = Some(&cancel);
    let result = analyze(&history, &Classical, limits, Config::default());
    assert_eq!(result.stop, "cancelled");
    limits.cancel = None;
    limits.hard = Some(Instant::now());
    let result = analyze(&history, &Classical, limits, Config::default());
    assert_eq!(result.stop, "hard_deadline");
    limits.hard = Some(Instant::now() + Duration::from_millis(15));
    let start = Instant::now();
    let result = analyze(&history, &Classical, limits, Config::default());
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(result.stop, "hard_deadline");
    assert_eq!(history.current(), Position::start());
}

#[test]
fn evaluation_reflection_and_terminal_precedence() {
    for fen in ["B:B9,10,K14:W21,22,K27", "W:B12:WK3", "B:BK1:W"] {
        let position: Position = fen.parse().unwrap();
        assert_eq!(features(position), features(position.flipped()));
        assert_eq!(
            Classical.evaluate(position),
            Classical.evaluate(position.flipped())
        );
    }
    struct Broken;
    impl Evaluator for Broken {
        fn evaluate(&self, _: Position) -> i32 {
            -1_000_000
        }
    }
    let terminal = History::new("B:BK1:W".parse().unwrap());
    let result = analyze(
        &terminal,
        &Broken,
        Limits::fixed(4, 1000),
        Config::default(),
    );
    assert_eq!(result.score, Some(30_000));
    assert_eq!(result.best, None);
}
