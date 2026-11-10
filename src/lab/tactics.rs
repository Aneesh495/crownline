use super::Rng;
use crate::{
    board::{Position, Side},
    eval::Classical,
    game::History,
    moves, oracle,
    search::{self, Config, Limits, Rules},
};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Serialize)]
pub struct Case {
    fen: String,
    motif: &'static str,
    legal: Vec<String>,
    oracle_score: i32,
    baseline_score: i32,
    selective_score: i32,
    baseline_nodes: u64,
    selective_nodes: u64,
}

const MOTIFS: [&str; 5] = [
    "different_capture_lengths",
    "branch_capture",
    "capture_crowning",
    "immobility",
    "quiet_tactical_reply",
];
const CASES_PER_MOTIF: usize = 44;

pub fn run() -> Result<Vec<Case>, String> {
    let mut rng = Rng(0xb1ca_7a05_1e42_b887);
    let mut seen = HashSet::new();
    let mut counts = [0; MOTIFS.len()];
    let mut cases = Vec::with_capacity(MOTIFS.len() * CASES_PER_MOTIF);
    let mut legal = Vec::new();
    let mut reply = Vec::new();
    for _ in 0..350_000 {
        let side = Side::ALL[rng.sample() as usize % 2];
        let n = 4 + (rng.sample() % 9);
        let mut masks = [0; 2];
        let mut kings = 0;
        for _ in 0..n {
            let square = (rng.sample() % 32) as u8;
            let owner = Side::ALL[rng.sample() as usize % 2];
            let flag = 1 << square;
            if (masks[0] | masks[1]) & flag != 0 {
                continue;
            }
            masks[owner as usize] |= flag;
            if rng.sample() % 3 == 0 || owner.crown(square) {
                kings |= flag;
            }
        }
        if masks.contains(&0) {
            continue;
        }
        let position = Position::new(masks[0], masks[1], kings, side)?;
        let canonical = if side == Side::Black {
            position
        } else {
            position.flipped()
        };
        if !seen.insert(canonical.to_string()) {
            continue;
        }
        oracle::generate(position, &mut legal);
        let motif = if legal.is_empty() {
            3
        } else if legal[0].is_capture() {
            if legal
                .iter()
                .any(|turn| turn.path().len() != legal[0].path().len())
            {
                0
            } else if legal.len() > 1 {
                1
            } else if side.crown(legal[0].to()) && position.kings() & (1 << legal[0].from()) == 0 {
                2
            } else {
                continue;
            }
        } else if legal.iter().any(|&turn| {
            oracle::generate(oracle::advance(position, turn), &mut reply);
            reply.first().is_some_and(|turn| turn.is_capture())
        }) {
            4
        } else {
            continue;
        };
        if counts[motif] >= CASES_PER_MOTIF {
            continue;
        }
        let fast = moves::legal(position);
        if fast.len() != legal.len() || fast.iter().any(|turn| !legal.contains(turn)) {
            return Err(format!("legal routes disagree at {position}"));
        }
        let history = History::new(position);
        let reference = search::minimax(&history, &Classical, 5, Rules::House);
        let limits = Limits::fixed(5, 180_000);
        let config = Config {
            table_entries: 512,
            ..Config::default()
        };
        let baseline = search::analyze(&history, &Classical, limits, config);
        let selective = search::analyze(
            &history,
            &Classical,
            limits,
            Config {
                selective: true,
                ..config
            },
        );
        if baseline.score != Some(reference)
            || baseline.abandoned.is_some()
            || selective.abandoned.is_some()
        {
            return Err(format!("minimax or budget discrepancy at {position}"));
        }
        counts[motif] += 1;
        cases.push(Case {
            fen: position.to_string(),
            motif: MOTIFS[motif],
            legal: legal.iter().map(ToString::to_string).collect(),
            oracle_score: reference,
            baseline_score: baseline.score.unwrap(),
            selective_score: selective.score.ok_or("selective search omitted score")?,
            baseline_nodes: baseline.nodes,
            selective_nodes: selective.nodes,
        });
        if counts.iter().all(|&n| n == CASES_PER_MOTIF) {
            return Ok(cases);
        }
    }
    Err(format!("tactical corpus did not fill quotas: {counts:?}"))
}
