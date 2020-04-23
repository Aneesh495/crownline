pub mod arena;
pub mod model;
pub mod selfplay;
pub mod train;

use crate::{
    board::{Position, Side, Turn},
    game::{History, Outcome, Reason, board_outcome},
    moves,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
};

pub const GAME_LIMIT: usize = 160;
pub const DATA_LIMIT: u64 = 192_000_000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rng(pub u64);

impl Rng {
    pub fn sample(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn uniform(&mut self) -> f64 {
        (self.sample() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn file_hash(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > DATA_LIMIT {
        return Err("artifact exceeds storage cap".into());
    }
    let mut hasher = Sha256::new();
    std::io::copy(
        &mut Read::by_ref(&mut file).take(DATA_LIMIT + 1),
        &mut hasher,
    )
    .map_err(|e| e.to_string())?;
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn read_lines<T: DeserializeOwned>(path: &Path, max: usize) -> Result<Vec<T>, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > DATA_LIMIT {
        return Err("dataset exceeds storage cap".into());
    }
    let mut reader = BufReader::new(file);
    let mut result = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .by_ref()
            .take(64 * 1024)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        if read == 64 * 1024 || result.len() >= max || !line.ends_with('\n') {
            return Err("invalid ledger boundary or sample limit exceeded".into());
        }
        result.push(
            serde_json::from_str(&line).map_err(|e| format!("line {}: {e}", result.len() + 1))?,
        );
    }
    Ok(result)
}

pub fn append(writer: &mut impl Write, record: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, record).map_err(|e| e.to_string())?;
    writer.write_all(b"\n").map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    Train,
    Validation,
    Test,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Opening {
    pub id: String,
    pub group: String,
    pub prefix: Vec<String>,
    pub split: Split,
}

impl Opening {
    pub fn history(&self) -> Result<History, String> {
        let mut history = History::new(Position::start());
        for text in &self.prefix {
            let turn = crate::board::resolve_turn(history.current(), text)?;
            history.play(turn)?;
        }
        Ok(history)
    }
}

pub fn openings() -> Vec<Opening> {
    fn step(position: Position, prefix: &mut Vec<String>, out: &mut Vec<Opening>) {
        if prefix.len() == 3 {
            let group = prefix[..2].join(" ");
            let hash = Sha256::digest(group.as_bytes());
            let split = match hash[0] % 10 {
                0 | 1 => Split::Test,
                2 | 3 => Split::Validation,
                _ => Split::Train,
            };
            out.push(Opening {
                id: digest(prefix.join(" ").as_bytes())[..16].into(),
                group,
                prefix: prefix.clone(),
                split,
            });
            return;
        }
        let mut turns = moves::legal(position);
        turns.sort_by(|a, b| a.path().cmp(b.path()));
        for turn in turns {
            prefix.push(turn.to_string());
            let mut next = position;
            next.make(turn);
            step(next, prefix, out);
            prefix.pop();
        }
    }
    let mut out = Vec::new();
    step(Position::start(), &mut Vec::new(), &mut out);
    out
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Finish {
    Elimination,
    Immobility,
    Repetition,
    NoProgress,
    PlyCap,
    Illegal,
    Crash,
    Timeout,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ending {
    pub winner: Option<Side>,
    pub reason: Finish,
}

fn ending(outcome: Outcome) -> Ending {
    let reason = match outcome.reason {
        Reason::Elimination => Finish::Elimination,
        Reason::Immobility => Finish::Immobility,
        Reason::Repetition => Finish::Repetition,
        Reason::NoProgress => Finish::NoProgress,
        _ => unreachable!(),
    };
    Ending {
        winner: outcome.winner,
        reason,
    }
}

pub fn play(
    opening: &Opening,
    limit: usize,
    mut select: impl FnMut(&History, &[Turn], usize) -> Result<Turn, Finish>,
) -> Result<(Vec<String>, Ending), String> {
    let mut history = opening.history()?;
    let mut turns = Vec::new();
    for ply in 0..=limit {
        let position = history.current();
        let legal = moves::legal(position);
        if let Some(outcome) = board_outcome(position, &legal).or_else(|| history.house_draw()) {
            return Ok((turns, ending(outcome)));
        }
        if ply == limit {
            break;
        }
        match select(&history, &legal, ply) {
            Ok(turn) if legal.contains(&turn) => {
                turns.push(turn.to_string());
                let mut next = position;
                next.make(turn);
                history.push_generated(turn, next);
            }
            Ok(_) => {
                return Ok((
                    turns,
                    Ending {
                        winner: Some(position.side().other()),
                        reason: Finish::Illegal,
                    },
                ));
            }
            Err(reason) => {
                return Ok((
                    turns,
                    Ending {
                        winner: Some(position.side().other()),
                        reason,
                    },
                ));
            }
        }
    }
    Ok((
        turns,
        Ending {
            winner: None,
            reason: Finish::PlyCap,
        },
    ))
}
pub mod reference;
pub mod tactics;
pub mod verify;
