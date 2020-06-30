use super::{
    digest, file_hash,
    model::{Model, ModelKind, NativeModel},
    read_lines,
    selfplay::GameRecord,
};
use crate::{
    board::{Position, Side},
    eval::{Classical, Evaluator},
    game::History,
    pdn,
    search::{Config, Limits, analyze},
    tablebase::atomic,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io, path::Path};

pub const SCHEMA: &str = "crownline-historical-v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub file: String,
    pub url: String,
    pub sha256: String,
    pub bytes: usize,
    pub records: usize,
    pub capture_notation: String,
    pub result_order: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelIdentity {
    pub seed: u64,
    pub sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub model_source_commit: String,
    pub training_sha256: String,
    pub teacher_nodes: u64,
    pub teacher_depth: u8,
    pub sample_plies: Vec<usize>,
    pub sources: Vec<Source>,
    pub models: Vec<ModelIdentity>,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GameReplay {
    pub source: String,
    pub index: usize,
    pub offset: usize,
    pub end: usize,
    pub sha256: String,
    pub turns: usize,
    pub result: String,
    pub final_fen: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelScore {
    pub seed: u64,
    pub linear: i32,
    pub network: i32,
    pub blend: i32,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub source: String,
    pub game: usize,
    pub ply: usize,
    pub fen: String,
    pub teacher: i32,
    pub teacher_depth: u8,
    pub teacher_nodes: u64,
    pub classical: i32,
    pub models: Vec<ModelScore>,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    pub seed: u64,
    pub linear_rmse: f64,
    pub network_rmse: f64,
    pub blend_rmse: f64,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema: String,
    pub manifest_sha256: String,
    pub model_source_commit: String,
    pub requested_games: usize,
    pub accepted_games: usize,
    pub rejected_games: usize,
    pub turns_replayed: usize,
    pub samples: usize,
    pub removed_training_overlap: usize,
    pub removed_duplicates: usize,
    pub classical_rmse: f64,
    pub models: Vec<Metrics>,
}

pub fn load_manifest(path: &Path) -> Result<Manifest, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > 32_768 {
        return Err("historical manifest exceeds cap".into());
    }
    let manifest: Manifest = serde_json::from_reader(file).map_err(|e| e.to_string())?;
    if manifest.schema != SCHEMA
        || manifest.sources.is_empty()
        || manifest.sources.len() > 8
        || manifest.models.len() != 3
        || manifest.teacher_nodes == 0
        || manifest.teacher_nodes > 65_536
        || !(1..=20).contains(&manifest.teacher_depth)
        || manifest.sample_plies.is_empty()
        || manifest.sample_plies.len() > 16
        || !manifest
            .sample_plies
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    {
        return Err("invalid historical manifest dimensions".into());
    }
    let mut files = HashSet::new();
    for source in &manifest.sources {
        if !files.insert(&source.file)
            || source.file.is_empty()
            || source.file.contains(['/', '\\'])
            || source.file.starts_with('.')
            || source.bytes == 0
            || source.bytes > 2_000_000
            || source.records == 0
            || source.records > 4096
            || !matches!(source.capture_notation.as_str(), "Standard" | "LegacyDash")
            || !matches!(source.result_order.as_str(), "BlackFirst" | "WhiteFirst")
        {
            return Err("invalid historical source declaration".into());
        }
    }
    let unique = manifest
        .models
        .iter()
        .map(|model| model.seed)
        .collect::<HashSet<_>>();
    if unique.len() != manifest.models.len() {
        return Err("historical model seeds must be distinct".into());
    }
    Ok(manifest)
}

pub fn source_text(source: &Source, directory: &Path) -> Result<String, String> {
    let path = directory.join(&source.file);
    if path.metadata().map_err(|e| e.to_string())?.len() != source.bytes as u64
        || file_hash(&path)? != source.sha256
    {
        return Err(format!(
            "historical source identity mismatch: {}",
            source.file
        ));
    }
    fs::read_to_string(path).map_err(|e| e.to_string())
}

pub fn declared_input(source: &Source, text: &str) -> String {
    format!(
        "[CaptureNotation \"{}\"][ResultOrder \"{}\"]\n{}",
        source.capture_notation, source.result_order, text,
    )
}

pub fn board_identity(position: Position) -> String {
    if position.side() == Side::Black {
        position
    } else {
        position.flipped()
    }
    .to_string()
}

pub fn frozen_models(manifest: &Manifest, directory: &Path) -> Result<Vec<Model>, String> {
    manifest
        .models
        .iter()
        .map(|identity| {
            let path = directory.join(format!("seed-{}/model.json", identity.seed));
            if file_hash(&path)? != identity.sha256 {
                return Err("historical model identity mismatch".into());
            }
            let model = Model::load(&path)?;
            if model.seed != identity.seed || model.data_sha256 != manifest.training_sha256 {
                return Err("historical model training identity mismatch".into());
            }
            Ok(model)
        })
        .collect()
}

pub fn training_boards(manifest: &Manifest, path: &Path) -> Result<HashSet<String>, String> {
    if file_hash(path)? != manifest.training_sha256 {
        return Err("historical training exclusion identity mismatch".into());
    }
    let games: Vec<GameRecord> = read_lines(path, 4096)?;
    let mut boards = HashSet::new();
    for game in games {
        let mut history = game.opening.history()?;
        boards.insert(board_identity(history.current()));
        for text in game.moves {
            let turn = crate::board::resolve_turn(history.current(), &text)?;
            history.play(turn)?;
            boards.insert(board_identity(history.current()));
        }
    }
    if boards.is_empty() {
        return Err("empty historical training exclusion set".into());
    }
    Ok(boards)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    atomic(path, |file| {
        serde_json::to_writer_pretty(file, value).map_err(io::Error::other)
    })
    .map_err(|e| e.to_string())
}

pub fn run(
    manifest_path: &Path,
    sources: &Path,
    training: &Path,
    model_dir: &Path,
    output: &Path,
) -> Result<Report, String> {
    let manifest = load_manifest(manifest_path)?;
    let exclusions = training_boards(&manifest, training)?;
    let models = frozen_models(&manifest, model_dir)?;
    let evaluators = models
        .iter()
        .map(|model| {
            [ModelKind::Linear, ModelKind::Network, ModelKind::Blend]
                .map(|kind| NativeModel::new(model, kind))
        })
        .collect::<Vec<_>>();
    fs::create_dir(output)
        .map_err(|e| format!("historical run requires a fresh directory: {e}"))?;
    let mut games = Vec::new();
    let mut probes = Vec::new();
    let mut seen = HashSet::new();
    let mut overlaps = 0;
    let mut duplicates = 0;
    for source in &manifest.sources {
        let input = source_text(source, sources)?;
        let ranges = pdn::archive_ranges(&input).map_err(|e| e.to_string())?;
        if ranges.len() != source.records {
            return Err(format!("historical record count mismatch: {}", source.file));
        }
        for (index, range) in ranges.into_iter().enumerate() {
            let raw = &input[range.clone()];
            let mut game = GameReplay {
                source: source.file.clone(),
                index,
                offset: range.start,
                end: range.end,
                sha256: digest(raw.as_bytes()),
                turns: 0,
                result: String::new(),
                final_fen: None,
                error: None,
            };
            match pdn::import(&declared_input(source, raw), false) {
                Err(error) => game.error = Some(error.to_string()),
                Ok(record) => {
                    game.turns = record.line.plies.len();
                    game.result = record.result;
                    game.final_fen = Some(
                        record
                            .line
                            .plies
                            .last()
                            .map_or(record.start, |ply| ply.after)
                            .to_string(),
                    );
                    let mut history = History::new(record.start);
                    for (ply, turn) in record.line.plies.iter().enumerate() {
                        if manifest.sample_plies.contains(&ply) {
                            let current = history.current();
                            let key = board_identity(current);
                            if exclusions.contains(&key) {
                                overlaps += 1;
                            } else if !seen.insert(key) {
                                duplicates += 1;
                            } else {
                                let teacher = analyze(
                                    &history,
                                    &Classical,
                                    Limits::fixed(manifest.teacher_depth, manifest.teacher_nodes),
                                    Config {
                                        table_entries: 512,
                                        ..Config::default()
                                    },
                                );
                                let score = teacher
                                    .score
                                    .ok_or("historical teacher did not complete a depth")?;
                                let depth = teacher.depths.last().map_or(0, |d| d.depth);
                                probes.push(Probe {
                                    source: source.file.clone(),
                                    game: index,
                                    ply,
                                    fen: current.to_string(),
                                    teacher: score,
                                    teacher_depth: depth,
                                    teacher_nodes: teacher.nodes,
                                    classical: Classical.evaluate(current),
                                    models: evaluators
                                        .iter()
                                        .zip(&models)
                                        .map(|(engines, model)| ModelScore {
                                            seed: model.seed,
                                            linear: engines[0].evaluate(current),
                                            network: engines[1].evaluate(current),
                                            blend: engines[2].evaluate(current),
                                        })
                                        .collect(),
                                });
                            }
                        }
                        history.push_generated(turn.turn, turn.after);
                    }
                }
            }
            games.push(game);
        }
    }
    if probes.is_empty() {
        return Err("historical workload produced zero eligible samples".into());
    }
    let rmse = |values: Vec<i32>| {
        (values
            .into_iter()
            .zip(&probes)
            .map(|(score, probe)| f64::from(score - probe.teacher).powi(2))
            .sum::<f64>()
            / probes.len() as f64)
            .sqrt()
    };
    let metrics = models
        .iter()
        .enumerate()
        .map(|(i, model)| Metrics {
            seed: model.seed,
            linear_rmse: rmse(probes.iter().map(|p| p.models[i].linear).collect()),
            network_rmse: rmse(probes.iter().map(|p| p.models[i].network).collect()),
            blend_rmse: rmse(probes.iter().map(|p| p.models[i].blend).collect()),
        })
        .collect();
    let accepted = games.iter().filter(|game| game.error.is_none()).count();
    let report = Report {
        schema: SCHEMA.into(),
        manifest_sha256: file_hash(manifest_path)?,
        model_source_commit: manifest.model_source_commit,
        requested_games: games.len(),
        accepted_games: accepted,
        rejected_games: games.len() - accepted,
        turns_replayed: games.iter().map(|g| g.turns).sum(),
        samples: probes.len(),
        removed_training_overlap: overlaps,
        removed_duplicates: duplicates,
        classical_rmse: rmse(probes.iter().map(|p| p.classical).collect()),
        models: metrics,
    };
    write_json(&output.join("games.json"), &games)?;
    write_json(&output.join("probes.json"), &probes)?;
    write_json(&output.join("report.json"), &report)?;
    Ok(report)
}
