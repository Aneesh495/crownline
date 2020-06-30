use super::{
    file_hash,
    historical::{self, GameReplay, Manifest, Probe, Report},
    model::{self, ModelKind, NativeModel},
    read_lines,
    selfplay::GameRecord,
};
use crate::{
    board::{Position, Turn},
    eval::{Classical, Evaluator},
    game::History,
    oracle, pdn,
    search::{self, Config, Limits},
};
use serde::de::DeserializeOwned;
use std::{collections::HashSet, fs, path::Path};

fn json<T: DeserializeOwned>(path: &Path, cap: u64) -> Result<T, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > cap {
        return Err("historical verification artifact exceeds cap".into());
    }
    serde_json::from_reader(file).map_err(|e| e.to_string())
}

fn replay(history: &mut History, text: &str, legal: &mut Vec<Turn>) -> Result<(), String> {
    oracle::generate(history.current(), legal);
    let turn = legal
        .iter()
        .copied()
        .find(|turn| turn.to_string() == text)
        .ok_or_else(|| format!("historical independent replay rejected {text}"))?;
    let next = oracle::advance(history.current(), turn);
    history.push_generated(turn, next);
    Ok(())
}

fn excluded(manifest: &Manifest, path: &Path) -> Result<HashSet<String>, String> {
    if file_hash(path)? != manifest.training_sha256 {
        return Err("historical exclusion dataset identity mismatch".into());
    }
    let games: Vec<GameRecord> = read_lines(path, 4096)?;
    let mut boards = HashSet::new();
    let mut legal = Vec::new();
    for game in games {
        let mut history = History::new(Position::start());
        boards.insert(historical::board_identity(history.current()));
        for text in game.opening.prefix.iter().chain(&game.moves) {
            replay(&mut history, text, &mut legal)?;
            boards.insert(historical::board_identity(history.current()));
        }
    }
    Ok(boards)
}

pub fn verify(
    manifest_path: &Path,
    sources: &Path,
    training: &Path,
    model_dir: &Path,
    output: &Path,
) -> Result<serde_json::Value, String> {
    let manifest = historical::load_manifest(manifest_path)?;
    let exclusions = excluded(&manifest, training)?;
    let models = historical::frozen_models(&manifest, model_dir)?;
    let games: Vec<GameReplay> = json(&output.join("games.json"), 2_000_000)?;
    let probes: Vec<Probe> = json(&output.join("probes.json"), 4_000_000)?;
    let report: Report = json(&output.join("report.json"), 32_768)?;
    if report.schema != historical::SCHEMA
        || report.manifest_sha256 != file_hash(manifest_path)?
        || report.model_source_commit != manifest.model_source_commit
        || probes.is_empty()
    {
        return Err("historical report identity mismatch".into());
    }
    let mut game_cursor = 0;
    let mut probe_cursor = 0;
    let mut turns = 0;
    let mut accepted = 0;
    let mut overlaps = 0;
    let mut duplicates = 0;
    let mut seen = HashSet::new();
    let mut legal = Vec::new();
    for source in &manifest.sources {
        let input = historical::source_text(source, sources)?;
        let ranges = pdn::archive_ranges(&input).map_err(|e| e.to_string())?;
        if ranges.len() != source.records {
            return Err("historical frozen source workload mismatch".into());
        }
        for (index, range) in ranges.into_iter().enumerate() {
            let raw = &input[range.clone()];
            let observed = games
                .get(game_cursor)
                .ok_or("historical game observation missing")?;
            game_cursor += 1;
            if observed.source != source.file
                || observed.index != index
                || observed.offset != range.start
                || observed.end != range.end
                || observed.sha256 != super::digest(raw.as_bytes())
            {
                return Err("historical game byte identity mismatch".into());
            }
            let record = match pdn::import(&historical::declared_input(source, raw), false) {
                Ok(record) => {
                    if observed.error.is_some()
                        || observed.result != record.result
                        || observed.turns != record.line.plies.len()
                    {
                        return Err("historical accepted-game observation mismatch".into());
                    }
                    record
                }
                Err(error) => {
                    if observed.error.as_deref() != Some(error.to_string().as_str())
                        || observed.turns != 0
                        || !observed.result.is_empty()
                        || observed.final_fen.is_some()
                    {
                        return Err("historical rejected-game observation mismatch".into());
                    }
                    continue;
                }
            };
            accepted += 1;
            let mut history = History::new(record.start);
            for (ply, played) in record.line.plies.iter().enumerate() {
                if manifest.sample_plies.contains(&ply) {
                    let position = history.current();
                    let key = historical::board_identity(position);
                    if exclusions.contains(&key) {
                        overlaps += 1;
                    } else if !seen.insert(key) {
                        duplicates += 1;
                    } else {
                        let probe = probes
                            .get(probe_cursor)
                            .ok_or("historical frozen probe missing")?;
                        probe_cursor += 1;
                        if probe.source != source.file
                            || probe.game != index
                            || probe.ply != ply
                            || probe.fen != position.to_string()
                            || probe.models.len() != models.len()
                            || probe.classical != Classical.evaluate(position)
                        {
                            return Err("historical probe position or workload mismatch".into());
                        }
                        let target = search::analyze(
                            &history,
                            &Classical,
                            Limits::fixed(manifest.teacher_depth, manifest.teacher_nodes),
                            Config {
                                table_entries: 512,
                                ..Config::default()
                            },
                        );
                        if target.score != Some(probe.teacher)
                            || target.nodes != probe.teacher_nodes
                            || target.depths.last().map_or(0, |d| d.depth) != probe.teacher_depth
                        {
                            return Err("historical teacher target replay mismatch".into());
                        }
                        for (observed, model) in probe.models.iter().zip(&models) {
                            let linear =
                                NativeModel::new(model, ModelKind::Linear).evaluate(position);
                            let network = model::exported_network(position, model);
                            if observed.seed != model.seed
                                || observed.linear != linear
                                || observed.network != network
                                || observed.blend != (linear + network) / 2
                                || network
                                    != NativeModel::new(model, ModelKind::Network)
                                        .evaluate(position)
                            {
                                return Err(
                                    "historical model score or exported inference mismatch".into(),
                                );
                            }
                        }
                    }
                }
                if history.current() != played.before {
                    return Err("historical prior board mismatch".into());
                }
                replay(&mut history, &played.turn.to_string(), &mut legal)?;
                if history.current() != played.after {
                    return Err("historical transition disagrees with array oracle".into());
                }
                turns += 1;
            }
            if observed.final_fen.as_deref() != Some(history.current().to_string().as_str()) {
                return Err("historical final position mismatch".into());
            }
        }
    }
    if game_cursor != games.len()
        || probe_cursor != probes.len()
        || report.samples != probe_cursor
        || report.requested_games != game_cursor
        || report.accepted_games != accepted
        || report.rejected_games != game_cursor - accepted
        || report.turns_replayed != turns
        || report.removed_training_overlap != overlaps
        || report.removed_duplicates != duplicates
        || report.models.len() != models.len()
    {
        return Err("historical accounting does not match raw replay".into());
    }
    let mean_error = |scores: Vec<i32>| {
        let squared: u64 = scores
            .iter()
            .zip(&probes)
            .map(|(score, target)| i64::from(score - target.teacher).pow(2) as u64)
            .sum();
        (squared as f64 / probes.len() as f64).sqrt()
    };
    if (report.classical_rmse - mean_error(probes.iter().map(|probe| probe.classical).collect()))
        .abs()
        > 1e-10
    {
        return Err("historical classical RMSE mismatch".into());
    }
    for (i, metric) in report.models.iter().enumerate() {
        if metric.seed != models[i].seed
            || (metric.linear_rmse
                - mean_error(probes.iter().map(|p| p.models[i].linear).collect()))
            .abs()
                > 1e-10
            || (metric.network_rmse
                - mean_error(probes.iter().map(|p| p.models[i].network).collect()))
            .abs()
                > 1e-10
            || (metric.blend_rmse - mean_error(probes.iter().map(|p| p.models[i].blend).collect()))
                .abs()
                > 1e-10
        {
            return Err("historical learned RMSE mismatch".into());
        }
    }
    Ok(serde_json::json!({
        "games_checked": game_cursor, "accepted": accepted, "turns_replayed": turns,
        "teacher_probes_checked": probe_cursor, "model_inferences_checked": probe_cursor * models.len() * 3,
        "training_overlap": overlaps, "duplicates": duplicates,
    }))
}
