use super::{
    Ending, Finish, GAME_LIMIT, Opening, Rng, Split, append, digest, openings, play, read_lines,
};
use crate::{
    board::{Position, Side},
    eval::Classical,
    game::History,
    search::{Config, Limits, analyze},
    tablebase::atomic,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, BufWriter},
    path::Path,
};

pub const SCHEMA: &str = "crownline-selfplay-v1";
const SEED: u64 = 0x6237_44bc_a7e1_d95a;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub ply: usize,
    pub fen: String,
    pub history: String,
    pub teacher: i32,
    pub teacher_nodes: u64,
    pub teacher_depth: u8,
    pub target: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameRecord {
    pub schema: String,
    pub id: usize,
    pub opening: Opening,
    pub rng_seed: u64,
    pub moves: Vec<String>,
    pub ending: Ending,
    pub labels: Vec<Label>,
}

fn target(teacher: i32, ending: &Ending, mover: Side) -> i32 {
    let teacher = teacher.clamp(-400, 400);
    if ending.reason == Finish::PlyCap {
        return teacher;
    }
    let result = ending
        .winner
        .map_or(0, |side| if side == mover { 150 } else { -150 });
    (3 * teacher + 2 * result) / 5
}

pub fn history_identity(history: &History) -> String {
    let mut words = String::new();
    for position in history.reversible() {
        let canonical = if history.current().side() == Side::White {
            position.flipped()
        } else {
            *position
        };
        words.push_str(&canonical.to_string());
        words.push('\n');
    }
    digest(words.as_bytes())
}

fn generate_one(id: usize, schedule: &[Opening]) -> Result<GameRecord, String> {
    let rng_seed = SEED ^ (id as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut rng = Rng(rng_seed);
    let opening = schedule[(id * 97 + 13) % schedule.len()].clone();
    let config = Config {
        table_entries: 512,
        ..Config::default()
    };
    let (moves, ending) = play(&opening, GAME_LIMIT, |history, legal, ply| {
        if ply < 40 && rng.sample() % 13 == 0 {
            return Ok(legal[rng.sample() as usize % legal.len()]);
        }
        analyze(history, &Classical, Limits::fixed(12, 256), config)
            .best
            .ok_or(Finish::Crash)
    })?;
    if matches!(
        ending.reason,
        Finish::Illegal | Finish::Crash | Finish::Timeout
    ) {
        return Err("self-play policy failed".into());
    }
    let mut history = opening.history()?;
    let mut labels = Vec::new();
    for (ply, text) in moves.iter().enumerate() {
        if ply % 9 == 3 && labels.len() < 12 {
            let evaluation = analyze(&history, &Classical, Limits::fixed(10, 2048), config);
            if let (Some(teacher), Some(depth)) = (evaluation.score, evaluation.depths.last()) {
                let current = history.current();
                labels.push(Label {
                    ply,
                    fen: current.to_string(),
                    history: history_identity(&history),
                    teacher,
                    teacher_nodes: evaluation.nodes,
                    teacher_depth: depth.depth,
                    target: target(teacher, &ending, current.side()),
                });
            }
        }
        let turn = crate::board::resolve_turn(history.current(), text)?;
        history.play(turn)?;
    }
    Ok(GameRecord {
        schema: SCHEMA.into(),
        id,
        opening,
        rng_seed,
        moves,
        ending,
        labels,
    })
}

pub fn generate(dir: &Path, games: usize) -> Result<serde_json::Value, String> {
    if !(1..=4096).contains(&games) {
        return Err("selfplay supports 1 through 4096 games per run".into());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let ledger = dir.join("selfplay.jsonl");
    let schedule = openings();
    let mut all = Vec::new();
    for id in 0..games {
        let path = dir.join(format!("game-{id:05}.json"));
        let game = if path.exists() {
            let metadata = path.metadata().map_err(|e| e.to_string())?;
            if metadata.len() > 64 * 1024 {
                return Err("oversized game checkpoint".into());
            }
            let game: GameRecord =
                serde_json::from_reader(fs::File::open(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if game.id != id
                || game.schema != SCHEMA
                || game.opening != schedule[(id * 97 + 13) % schedule.len()]
            {
                return Err(format!("checkpoint identity mismatch for game {id}"));
            }
            game
        } else {
            let game = generate_one(id, &schedule)?;
            atomic(&path, |file| {
                serde_json::to_writer(file, &game).map_err(io::Error::other)
            })
            .map_err(|e| e.to_string())?;
            game
        };
        all.push(game);
        if (id + 1) % 48 == 0 {
            eprintln!("self-play games committed: {}", id + 1);
        }
    }
    atomic(&ledger, |file| {
        let mut writer = BufWriter::new(file);
        for game in &all {
            append(&mut writer, game).map_err(io::Error::other)?;
        }
        std::io::Write::flush(&mut writer)
    })
    .map_err(|e| e.to_string())?;
    let datasets = samples(&ledger)?;
    Ok(
        serde_json::json!({"schema": SCHEMA, "games": games, "sha256": super::file_hash(&ledger)?,
        "positions_train": datasets.train.len(), "positions_validation": datasets.validation.len(),
        "positions_test": datasets.test.len(), "removed_cross_split": datasets.cross_split}),
    )
}

#[derive(Clone)]
pub struct Sample {
    pub position: Position,
    pub target: i32,
}

pub struct Dataset {
    pub train: Vec<Sample>,
    pub validation: Vec<Sample>,
    pub test: Vec<Sample>,
    pub cross_split: usize,
}

pub fn samples(ledger: &Path) -> Result<Dataset, String> {
    let games: Vec<GameRecord> = read_lines(ledger, 4096)?;
    let schedule = openings();
    let mut by_board: HashMap<String, u8> = HashMap::new();
    let mut rows = Vec::new();
    for (id, game) in games.iter().enumerate() {
        if game.id != id
            || game.schema != SCHEMA
            || game.labels.len() > 12
            || game.moves.len() > GAME_LIMIT
            || game.opening != schedule[(id * 97 + 13) % schedule.len()]
        {
            return Err(format!("invalid data identity for game {id}"));
        }
        let split = match game.opening.split {
            Split::Train => 1,
            Split::Validation => 2,
            Split::Test => 4,
        };
        for label in &game.labels {
            let position: Position = label.fen.parse()?;
            if label.ply >= game.moves.len()
                || label.teacher_nodes > 2048
                || label.teacher_depth == 0
                || label.target != target(label.teacher, &game.ending, position.side())
            {
                return Err(format!("invalid label at game {id}, ply {}", label.ply));
            }
            let board = if position.side() == Side::Black {
                position
            } else {
                position.flipped()
            };
            let board = board.to_string();
            *by_board.entry(board.clone()).or_default() |= split;
            rows.push((
                split,
                board,
                &label.history,
                Sample {
                    position,
                    target: label.target,
                },
            ));
        }
    }
    let mut seen = HashSet::new();
    let mut dataset = Dataset {
        train: Vec::new(),
        validation: Vec::new(),
        test: Vec::new(),
        cross_split: 0,
    };
    for (split, board, history, sample) in rows {
        if by_board[&board] != split {
            dataset.cross_split += 1;
            continue;
        }
        if !seen.insert((board, history)) {
            continue;
        }
        match split {
            1 => dataset.train.push(sample),
            2 => dataset.validation.push(sample),
            4 => dataset.test.push(sample),
            _ => unreachable!(),
        }
    }
    Ok(dataset)
}
