use crate::{
    board::{Position, Side},
    eval::Classical,
    game::History,
    moves,
    pdn::Record,
    search::{Config, Limits, analyze},
    tablebase::{Proof, Tablebase, Wdl},
};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Candidate {
    pub turn: String,
    pub after: String,
    pub proof_for_mover: Option<Wdl>,
    pub shallow: Option<i32>,
    pub deep: Option<i32>,
    pub deep_depth: u8,
    pub continuation: Vec<String>,
}

#[derive(Serialize)]
pub struct ReviewedTurn {
    pub ply: usize,
    pub side: Side,
    pub before: String,
    pub played: String,
    pub board_proof_before: Option<Proof>,
    pub mover_score_before: Option<i32>,
    pub mover_score_after: Option<i32>,
    pub black_score_after: Option<i32>,
    pub classification: &'static str,
    pub candidates: Vec<Candidate>,
}

#[derive(Serialize)]
pub struct Review {
    pub source_sha256: String,
    pub rules: &'static str,
    pub nodes_per_candidate_per_search: u64,
    pub rows: Vec<ReviewedTurn>,
}

fn opponent(value: Wdl) -> Wdl {
    match value {
        Wdl::Win => Wdl::Loss,
        Wdl::Loss => Wdl::Win,
        Wdl::Draw => Wdl::Draw,
    }
}

fn probe(table: Option<&Tablebase>, position: Position) -> Option<Proof> {
    table?.probe(position)
}

pub fn review(record: &Record, table: Option<&Tablebase>, nodes: u64) -> Result<Review, String> {
    if record.diagnostic.is_some() || !(32..=4096).contains(&nodes) || record.line.plies.len() > 256
    {
        return Err("review requires a complete record of at most 256 turns and a budget of 32 through 4096 nodes".into());
    }
    let mut history = History::new(record.start);
    let mut rows = Vec::new();
    for (index, played) in record.line.plies.iter().enumerate() {
        let position = history.current();
        if played.before != position {
            return Err("inconsistent PDN position during review".into());
        }
        let exact = probe(table, position);
        let legal = moves::legal(position);
        let mut candidates = Vec::with_capacity(legal.len());
        for turn in legal {
            let mut next = position;
            next.make(turn);
            history.push_generated(turn, next);
            let configuration = Config {
                table_entries: 1024,
                ..Config::default()
            };
            let shallow = analyze(&history, &Classical, Limits::fixed(2, nodes), configuration);
            let deep = analyze(&history, &Classical, Limits::fixed(4, nodes), configuration);
            history.pop();
            candidates.push(Candidate {
                turn: turn.to_string(),
                after: next.to_string(),
                proof_for_mover: probe(table, next).map(|value| opponent(value.wdl)),
                shallow: shallow.score.map(|s| -s),
                deep: deep.score.map(|s| -s),
                deep_depth: deep.depths.last().map_or(0, |d| d.depth),
                continuation: deep.pv.iter().map(ToString::to_string).collect(),
            });
        }
        candidates.sort_by(|a, b| b.deep.cmp(&a.deep).then(a.turn.cmp(&b.turn)));
        let played_text = played.turn.to_string();
        let actual = candidates
            .iter()
            .find(|candidate| candidate.turn == played_text)
            .ok_or("review turn was illegal")?;
        let best = candidates.first().ok_or("no candidates")?;
        let classification = match (exact.map(|p| p.wdl), actual.proof_for_mover) {
            (Some(Wdl::Win), Some(Wdl::Draw | Wdl::Loss)) => "board_only_lost_win",
            (Some(Wdl::Draw), Some(Wdl::Loss)) => "board_only_lost_draw",
            _ if best.deep.zip(actual.deep).is_some_and(|(a, b)| a - b >= 60)
                && best
                    .shallow
                    .zip(actual.shallow)
                    .is_some_and(|(a, b)| a - b >= 60)
                && best.deep_depth >= 3
                && actual.deep_depth >= 3 =>
            {
                "heuristic_drop"
            }
            _ => "no_flag",
        };
        rows.push(ReviewedTurn {
            ply: index + 1,
            side: position.side(),
            before: position.to_string(),
            played: played_text,
            board_proof_before: exact,
            mover_score_before: best.deep,
            mover_score_after: actual.deep,
            black_score_after: actual.deep.map(|s| {
                if position.side() == Side::Black {
                    s
                } else {
                    -s
                }
            }),
            classification,
            candidates,
        });
        history.push_generated(played.turn, played.after);
    }
    Ok(Review {
        source_sha256: record.sha256.clone(),
        rules: "estimates use house history; proofs use board-only infinite-play draws",
        nodes_per_candidate_per_search: nodes,
        rows,
    })
}

#[derive(Serialize)]
pub struct Witness {
    pub position: String,
    pub proof: Proof,
    pub chosen: Option<String>,
}

pub fn witness(
    mut position: Position,
    table: &Tablebase,
    max: usize,
) -> Result<Vec<Witness>, String> {
    if !(1..=256).contains(&max) {
        return Err("witness bound must be 1 through 256".into());
    }
    let mut result = Vec::new();
    for _ in 0..max {
        let proof = table
            .probe(position)
            .ok_or("position lies outside the closed tablebase")?;
        let mut candidates = Vec::new();
        for turn in moves::legal(position) {
            let mut next = position;
            next.make(turn);
            let child = table.probe(next).ok_or("witness encountered a frontier")?;
            if child.wdl == opponent(proof.wdl)
                && (proof.wdl == Wdl::Draw || child.witness_rank < proof.witness_rank)
            {
                candidates.push((turn, next, child));
            }
        }
        candidates.sort_by_key(|(_, _, child)| child.witness_rank);
        let candidate = if proof.wdl == Wdl::Loss {
            candidates.last()
        } else {
            candidates.first()
        };
        result.push(Witness {
            position: position.to_string(),
            proof,
            chosen: candidate.map(|(turn, _, _)| turn.to_string()),
        });
        if let Some((_, next, _)) = candidate {
            position = *next;
        } else {
            break;
        }
    }
    Ok(result)
}
