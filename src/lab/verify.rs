use super::{
    Ending, Finish, GAME_LIMIT, Opening, Split,
    arena::{self, Manifest, Match, Profile},
    file_hash,
    model::{Model, NativeModel},
    openings, read_lines,
    selfplay::{self, GameRecord},
    train::{Probe, check_probe},
};
use crate::{
    board::{Position, Side, Turn},
    eval::{Classical, Evaluator},
    game::History,
    oracle, search,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{fs, io::Read, path::Path};

fn read_json<T: DeserializeOwned>(path: &Path, cap: u64) -> Result<T, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > cap {
        return Err("verification input exceeds cap".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

struct Replay {
    history: History,
    legal: Vec<Turn>,
}

impl Replay {
    fn start() -> Self {
        let mut replay = Self {
            history: History::new(Position::start()),
            legal: Vec::with_capacity(32),
        };
        oracle::generate(replay.history.current(), &mut replay.legal);
        replay
    }

    fn apply(&mut self, text: &str) -> Result<(), String> {
        let position = self.history.current();
        let turn = self
            .legal
            .iter()
            .copied()
            .find(|turn| turn.to_string() == text)
            .ok_or_else(|| format!("illegal full-turn replay {text} at {position}"))?;
        let next = oracle::advance(position, turn);
        self.history.push_generated(turn, next);
        oracle::generate(next, &mut self.legal);
        Ok(())
    }

    fn terminal(&self) -> Option<Ending> {
        let position = self.history.current();
        let black = position.mask(Side::Black);
        let white = position.mask(Side::White);
        if black == 0 || white == 0 {
            return Some(Ending {
                winner: Some(if black == 0 { Side::White } else { Side::Black }),
                reason: Finish::Elimination,
            });
        }
        if self.legal.is_empty() {
            return Some(Ending {
                winner: Some(position.side().other()),
                reason: Finish::Immobility,
            });
        }
        let repetitions = self
            .history
            .reversible()
            .iter()
            .filter(|&&state| state == position)
            .count();
        if repetitions >= 3 {
            return Some(Ending {
                winner: None,
                reason: Finish::Repetition,
            });
        }
        if self.history.reversible().len() > 80 {
            return Some(Ending {
                winner: None,
                reason: Finish::NoProgress,
            });
        }
        None
    }
}

fn start(opening: &Opening) -> Result<Replay, String> {
    let mut replay = Replay::start();
    for turn in &opening.prefix {
        replay.apply(turn)?;
    }
    Ok(replay)
}

fn verify_ending(replay: &Replay, plies: usize, ending: &Ending) -> Result<(), String> {
    let derived = replay.terminal().or_else(|| {
        (plies == GAME_LIMIT).then_some(Ending {
            winner: None,
            reason: Finish::PlyCap,
        })
    });
    if derived.as_ref() != Some(ending) {
        return Err(format!(
            "recorded winner or result disagrees with legal replay: {ending:?} vs {derived:?}"
        ));
    }
    Ok(())
}

#[derive(Serialize)]
pub struct ArenaCheck {
    pub pairs: usize,
    pub games: usize,
    pub turns_replayed: u64,
    pub deterministic_decisions_checked: u64,
    pub challenger_wins: u64,
    pub challenger_losses: u64,
    pub drawn_games: u64,
    pub adjudications: u64,
}

pub fn verify_arena(model_path: &Path, directory: &Path) -> Result<ArenaCheck, String> {
    let manifest: Manifest = read_json(&directory.join("arena.json"), 16384)?;
    let model = Model::load(model_path)?;
    let actual_hash = file_hash(model_path)?;
    if manifest.schema != arena::SCHEMA
        || manifest.model_sha256 != actual_hash
        || manifest.model_seed != model.seed
        || manifest.split == Split::Train
        || manifest.ply_cap != GAME_LIMIT
    {
        return Err("arena model identity or profile mismatch".into());
    }
    let choices = arena::scheduled(&manifest.split);
    if manifest.pairs == 0 || manifest.pairs > choices.len() {
        return Err("pair workload outside frozen schedule".into());
    }
    let matches: Vec<Match> = read_lines(&directory.join("matches.jsonl"), 2 * choices.len())?;
    if matches.len() != 2 * manifest.pairs {
        return Err("missing color-paired games".into());
    }
    let mut report = ArenaCheck {
        pairs: manifest.pairs,
        games: matches.len(),
        turns_replayed: 0,
        deterministic_decisions_checked: 0,
        challenger_wins: 0,
        challenger_losses: 0,
        drawn_games: 0,
        adjudications: 0,
    };
    for (index, game) in matches.iter().enumerate() {
        let pair = index / 2;
        if game.pair != pair
            || game.challenger != Side::ALL[index % 2]
            || game.opening != choices[pair]
            || game.profile != manifest.profile
            || game.model_sha256 != actual_hash
            || game.moves.len() > GAME_LIMIT
            || game.search_nodes.iter().sum::<u64>() == 0
        {
            return Err(format!(
                "match identity, budget or color pairing differs at game {index}"
            ));
        }
        let mut replay = start(&game.opening)?;
        let net = NativeModel::new(&model, manifest.model_kind);
        for (ply, text) in game.moves.iter().enumerate() {
            if replay.terminal().is_some() {
                return Err(format!("turn beyond termination in game {index}"));
            }
            // Frozen probe indices are chosen before reading outcomes.
            if manifest.profile != Profile::Time1ms
                && index % 11 == 0
                && matches!(ply, 0 | 13 | 37 | 71)
            {
                let eval: &dyn Evaluator = if game.challenger == replay.history.current().side() {
                    &net
                } else {
                    &Classical
                };
                let check = search::analyze(
                    &replay.history,
                    eval,
                    manifest.profile.limits(),
                    arena::config(),
                );
                if check.best.is_none_or(|turn| turn.to_string() != *text) {
                    return Err(format!(
                        "fixed-node model replay differs at game {index} turn {ply}"
                    ));
                }
                report.deterministic_decisions_checked += 1;
            }
            replay.apply(text)?;
            report.turns_replayed += 1;
        }
        verify_ending(&replay, game.moves.len(), &game.ending)?;
        match game.ending.winner {
            Some(winner) if winner == game.challenger => report.challenger_wins += 1,
            Some(_) => report.challenger_losses += 1,
            None => report.drawn_games += 1,
        }
        report.adjudications += u64::from(game.ending.reason == Finish::PlyCap);
    }
    Ok(report)
}

pub fn verify_dataset(path: &Path) -> Result<serde_json::Value, String> {
    let games: Vec<GameRecord> = read_lines(path, 4096)?;
    if games.is_empty() {
        return Err("empty self-play dataset".into());
    }
    let schedule = openings();
    let mut turns = 0;
    let mut labels = 0;
    let mut teacher_probes = 0;
    for (id, game) in games.iter().enumerate() {
        if game.id != id
            || game.opening != schedule[(id * 97 + 13) % schedule.len()]
            || game.schema != selfplay::SCHEMA
        {
            return Err("self-play schedule mismatch".into());
        }
        let mut replay = start(&game.opening)?;
        let mut recorded = game.labels.iter().peekable();
        for (ply, text) in game.moves.iter().enumerate() {
            if replay.terminal().is_some() {
                return Err("self-play continued past result".into());
            }
            if recorded.peek().is_some_and(|label| label.ply == ply) {
                let label = recorded.next().unwrap();
                if ply % 9 != 3
                    || label.fen != replay.history.current().to_string()
                    || label.history != selfplay::history_identity(&replay.history)
                    || !(1..=2048).contains(&label.teacher_nodes)
                {
                    return Err(format!(
                        "wrong self-play position/label identity in game {id} turn {ply}"
                    ));
                }
                let teacher = label.teacher.clamp(-400, 400);
                let outcome = match game.ending.winner {
                    Some(winner) if winner == replay.history.current().side() => 150,
                    Some(_) => -150,
                    None => 0,
                };
                let target = if game.ending.reason == Finish::PlyCap {
                    teacher
                } else {
                    (3 * teacher + 2 * outcome) / 5
                };
                if target != label.target {
                    return Err("target sign or outcome mismatch".into());
                }
                if id % 13 == 0 && matches!(ply, 3 | 30 | 57) {
                    let evaluation = search::analyze(
                        &replay.history,
                        &Classical,
                        search::Limits::fixed(10, 2048),
                        arena::config(),
                    );
                    if evaluation.score != Some(label.teacher)
                        || evaluation.nodes != label.teacher_nodes
                        || evaluation.depths.last().map(|depth| depth.depth)
                            != Some(label.teacher_depth)
                    {
                        return Err("teacher probe differs from frozen native search".into());
                    }
                    teacher_probes += 1;
                }
                labels += 1;
            }
            replay.apply(text)?;
            turns += 1;
        }
        if recorded.next().is_some() {
            return Err("unordered or orphan training label".into());
        }
        verify_ending(&replay, game.moves.len(), &game.ending)?;
    }
    let split = selfplay::samples(path)?;
    Ok(serde_json::json!({
        "games": games.len(), "turns_replayed": turns, "labels": labels, "teacher_probes": teacher_probes,
        "train": split.train.len(), "validation": split.validation.len(), "test": split.test.len(),
        "cross_split_removed": split.cross_split,
    }))
}

pub fn verify_model(ledger: &Path, directory: &Path) -> Result<serde_json::Value, String> {
    let model = Model::load(&directory.join("model.json"))?;
    if model.data_sha256 != file_hash(ledger)? {
        return Err("model training dataset identity mismatch".into());
    }
    let probes: Vec<Probe> = read_json(&directory.join("probes.json"), 16_384)?;
    let dataset = selfplay::samples(ledger)?;
    let expected = dataset
        .validation
        .iter()
        .step_by((dataset.validation.len() / 24).max(1))
        .take(24)
        .collect::<Vec<_>>();
    if probes.is_empty()
        || probes.len() != expected.len()
        || !probes.iter().any(|probe| probe.network != 0)
    {
        return Err("probe workload missing or identical to disabled model".into());
    }
    for (probe, expected) in probes.iter().zip(expected) {
        if probe.fen != expected.position.to_string() {
            return Err("probe does not match frozen validation example".into());
        }
        check_probe(probe, &model)?;
    }
    Ok(
        serde_json::json!({"seed": model.seed, "probes_checked": probes.len(), "data_sha256": model.data_sha256}),
    )
}

pub fn verify_reference(executable: &Path, directory: &Path) -> Result<serde_json::Value, String> {
    use super::reference::{self, Game, Manifest};
    let manifest: Manifest = read_json(&directory.join("reference.json"), 8192)?;
    let candidates = arena::scheduled(&Split::Test);
    let identity = file_hash(executable)?;
    if manifest.comparator_commit != reference::MARCHER_COMMIT
        || manifest.comparator_sha256 != identity
        || manifest.pairs == 0
        || manifest.pairs > candidates.len()
        || manifest.search_micros != reference::MICROS
        || manifest.ply_cap != GAME_LIMIT
    {
        return Err("reference binary identity or profile mismatch".into());
    }
    let games: Vec<Game> = read_lines(&directory.join("matches.jsonl"), 2 * candidates.len())?;
    if games.len() != 2 * manifest.pairs {
        return Err("reference arena lost required color-paired games".into());
    }
    let mut wins = 0;
    let mut losses = 0;
    let mut draws = 0;
    let mut caps = 0;
    let mut turns = 0;
    let mut overrun = [0; 2];
    for (index, game) in games.iter().enumerate() {
        let pair = index / 2;
        if game.pair != pair
            || game.crownline != Side::ALL[index % 2]
            || game.opening != candidates[pair]
            || game.moves.len() > GAME_LIMIT
        {
            return Err("reference game not in frozen paired schedule".into());
        }
        let mut replay = start(&game.opening)?;
        for text in &game.moves {
            if replay.terminal().is_some() {
                return Err("reference match continued after terminal".into());
            }
            replay.apply(text)?;
            turns += 1;
        }
        verify_ending(&replay, game.moves.len(), &game.ending)?;
        let slot = game.crownline as usize;
        overrun[0] += game.overruns_2x[slot];
        overrun[1] += game.overruns_2x[1 - slot];
        match game.ending.winner {
            Some(winner) if winner == game.crownline => wins += 1,
            Some(_) => losses += 1,
            None => draws += 1,
        }
        caps += usize::from(game.ending.reason == Finish::PlyCap);
    }
    Ok(
        serde_json::json!({"wins": wins, "draws_including_caps": draws, "losses": losses,
        "caps": caps, "turns": turns, "overruns_2x_crownline_marcher": overrun, "games": games.len()}),
    )
}
