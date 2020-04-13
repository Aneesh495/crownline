use super::{
    Ending, Finish, GAME_LIMIT, Opening, Rng, Split, append, file_hash,
    model::{Model, ModelKind, NativeModel},
    openings, play,
};
use crate::{
    board::Side,
    eval::{Classical, Evaluator},
    search::{Config, Limits, analyze},
    tablebase::atomic,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufWriter, Write},
    path::Path,
    time::{Duration, Instant},
};

pub const SCHEMA: &str = "crownline-arena-v1";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Nodes192,
    Nodes768,
    Time1ms,
}

impl Profile {
    pub fn limits(self) -> Limits<'static> {
        match self {
            Self::Nodes192 => Limits::fixed(14, 192),
            Self::Nodes768 => Limits::fixed(14, 768),
            Self::Time1ms => {
                let now = Instant::now();
                Limits {
                    depth: 20,
                    nodes: 100_000,
                    soft: Some(now + Duration::from_micros(800)),
                    hard: Some(now + Duration::from_millis(1)),
                    cancel: None,
                }
            }
        }
    }
}

pub fn config() -> Config {
    Config {
        table_entries: 512,
        ..Config::default()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub model_sha256: String,
    pub model_seed: u64,
    pub model_kind: ModelKind,
    pub profile: Profile,
    pub split: Split,
    pub pairs: usize,
    pub ply_cap: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    pub pair: usize,
    pub challenger: Side,
    pub opening: Opening,
    pub profile: Profile,
    pub model_sha256: String,
    pub moves: Vec<String>,
    pub ending: Ending,
    pub search_nodes: [u64; 2],
    pub search_micros: [u64; 2],
    pub fallbacks: [u32; 2],
}

pub fn scheduled(split: &Split) -> Vec<Opening> {
    openings()
        .into_iter()
        .filter(|opening| &opening.split == split)
        .collect()
}

fn one(
    pair: usize,
    challenger: Side,
    opening: &Opening,
    manifest: &Manifest,
    model: &Model,
) -> Result<Match, String> {
    let engine = NativeModel::new(model, manifest.model_kind);
    let mut search_nodes = [0; 2];
    let mut search_micros = [0; 2];
    let mut fallbacks = [0; 2];
    let (moves, ending) = play(opening, GAME_LIMIT, |history, _, _| {
        let side = history.current().side();
        let slot = side as usize;
        let evaluator: &dyn Evaluator = if side == challenger {
            &engine
        } else {
            &Classical
        };
        let start = Instant::now();
        let result = analyze(history, evaluator, manifest.profile.limits(), config());
        search_micros[slot] += start.elapsed().as_micros() as u64;
        search_nodes[slot] += result.nodes;
        fallbacks[slot] += u32::from(result.depths.is_empty());
        result.best.ok_or(Finish::Crash)
    })?;
    Ok(Match {
        pair,
        challenger,
        opening: opening.clone(),
        profile: manifest.profile,
        model_sha256: manifest.model_sha256.clone(),
        moves,
        ending,
        search_nodes,
        search_micros,
        fallbacks,
    })
}

fn score(game: &Match) -> f64 {
    game.ending.winner.map_or(
        0.5,
        |winner| if winner == game.challenger { 1.0 } else { 0.0 },
    )
}

pub fn report(games: &[Match]) -> serde_json::Value {
    let mut wins = 0;
    let mut losses = 0;
    let mut draws = 0;
    let mut caps = 0;
    let mut technical = 0;
    let mut groups = BTreeMap::<&str, (f64, usize)>::new();
    for game in games {
        let value = score(game);
        match game.ending.reason {
            Finish::PlyCap => caps += 1,
            Finish::Illegal | Finish::Crash | Finish::Timeout => technical += 1,
            _ => {}
        }
        if value == 1.0 {
            wins += 1;
        } else if value == 0.0 {
            losses += 1;
        } else {
            draws += 1;
        }
        let cell = groups.entry(&game.opening.group).or_default();
        cell.0 += value - 0.5;
        cell.1 += 1;
    }
    let mean = (wins as f64 + 0.5 * draws as f64) / games.len() as f64 - 0.5;
    let units = groups.values().collect::<Vec<_>>();
    let mut rng = Rng(0xfde3_f21b_7ac5_99ed);
    let mut bootstrap = (0..5000)
        .map(|_| {
            let mut total = 0.0;
            let mut n = 0;
            for _ in &units {
                let &(x, count) = units[rng.sample() as usize % units.len()];
                total += x;
                n += count;
            }
            total / n as f64
        })
        .collect::<Vec<_>>();
    bootstrap.sort_unstable_by(f64::total_cmp);
    let interval = [bootstrap[125], bootstrap[4874]];
    serde_json::json!({
        "games": games.len(), "opening_pairs": games.len() / 2, "independent_opening_families": units.len(),
        "wins": wins, "draws_including_caps": draws, "losses": losses, "ply_cap_adjudications": caps,
        "technical_outcomes": technical, "gain_over_equal_score": mean, "cluster_bootstrap_95": interval,
        "promotion_eligible": technical == 0 && units.len() >= 8 && mean >= 0.02 && interval[0] > 0.0,
        "search_nodes": games.iter().map(|game| game.search_nodes.iter().sum::<u64>()).sum::<u64>(),
        "search_micros": games.iter().map(|game| game.search_micros.iter().sum::<u64>()).sum::<u64>(),
        "fallbacks": games.iter().map(|game| game.fallbacks.iter().sum::<u32>()).sum::<u32>(),
    })
}

pub fn run(
    model_path: &Path,
    directory: &Path,
    kind: ModelKind,
    profile: Profile,
    split: Split,
    pairs: usize,
) -> Result<serde_json::Value, String> {
    let model = Model::load(model_path)?;
    let candidates = scheduled(&split);
    if pairs == 0
        || pairs > candidates.len()
        || split == Split::Train
        || kind == ModelKind::Disabled
    {
        return Err(format!(
            "use 1 through {} unique validation/test openings and a trained evaluator",
            candidates.len()
        ));
    }
    fs::create_dir(directory).map_err(|e| format!("arena requires a fresh run directory: {e}"))?;
    let manifest = Manifest {
        schema: SCHEMA.into(),
        model_sha256: file_hash(model_path)?,
        model_seed: model.seed,
        model_kind: kind,
        profile,
        split,
        pairs,
        ply_cap: GAME_LIMIT,
    };
    atomic(&directory.join("arena.json"), |file| {
        serde_json::to_writer_pretty(file, &manifest).map_err(io::Error::other)
    })
    .map_err(|e| e.to_string())?;
    let ledger = directory.join("matches.jsonl");
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&ledger)
        .map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(file);
    let mut matches = Vec::with_capacity(2 * pairs);
    for (pair, opening) in candidates.iter().enumerate().take(pairs) {
        for side in Side::ALL {
            let result = one(pair, side, opening, &manifest, &model)?;
            append(&mut writer, &result)?;
            matches.push(result);
        }
        writer.flush().map_err(|e| e.to_string())?;
        if (pair + 1) % 24 == 0 {
            eprintln!("arena pairs committed: {}", pair + 1);
        }
    }
    writer.get_ref().sync_all().map_err(|e| e.to_string())?;
    let report = report(&matches);
    atomic(&directory.join("summary.json"), |file| {
        serde_json::to_writer_pretty(file, &report).map_err(io::Error::other)
    })
    .map_err(|e| e.to_string())?;
    Ok(report)
}
