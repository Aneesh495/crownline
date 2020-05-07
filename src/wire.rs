use crate::{
    board::{Position, Side, Turn},
    eval::Classical,
    game::{ClockSpec, Command, Game, Outcome, Policy},
    pdn::{self, Annotation, Line},
    review, search,
    tablebase::Tablebase,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAGIC: [u8; 4] = *b"CL01";
const INPUT_MAX: usize = 384 * 1024;
const OUTPUT_MAX: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    New {
        fen: Option<String>,
        policy: Policy,
        clock: Option<ClockSpec>,
        local: bool,
        opening: Option<Vec<String>>,
    },
    State,
    Act {
        actor: Side,
        revision: usize,
        command: Command,
    },
    Bot {
        revision: usize,
        millis: u64,
        nodes: u64,
        depth: u8,
    },
    Analyze {
        fen: Option<String>,
        pdn: Option<String>,
        millis: u64,
        nodes: u64,
        depth: u8,
    },
    Import {
        text: String,
        partial: bool,
    },
    Restore {
        text: String,
        policy: Policy,
    },
    Review {
        text: String,
        nodes: u64,
    },
    Witness {
        fen: String,
        max: usize,
    },
}

pub fn view_turn(turn: Turn) -> Value {
    json!({
        "text": turn.to_string(),
        "path": turn.path().iter().map(|s| s+1).collect::<Vec<_>>(),
        "capture": turn.is_capture(),
    })
}

fn result_token(outcome: Option<Outcome>) -> &'static str {
    match outcome.map(|o| o.winner) {
        None => "*",
        Some(None) => "1/2-1/2",
        Some(Some(Side::Black)) => "0-1",
        Some(Some(Side::White)) => "1-0",
    }
}

fn line_view(line: &Line) -> Value {
    json!({
        "introduction": line.introduction,
        "plies": line.plies.iter().map(|ply| json!({
            "turn": view_turn(ply.turn), "before": ply.before.to_string(), "after": ply.after.to_string(),
            "annotations": ply.annotations.iter().map(|annotation| match annotation {
                Annotation::Comment(text) => json!({"comment": text}),
                Annotation::Glyph(text) => json!({"glyph": text}),
                Annotation::Variation(branch) => json!({"variation": line_view(branch)}),
            }).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

pub fn result_view(result: &search::SearchResult) -> Value {
    json!({
        "best": result.best.map(view_turn), "score": result.score, "pv": result.pv.iter().map(|turn| view_turn(*turn)).collect::<Vec<_>>(),
        "depths": result.depths, "nodes": result.nodes, "cutoffs": result.cutoffs,
        "table_hits": result.table_hits, "proof_hits": result.proof_hits, "table_bytes": result.table_bytes, "abandoned": result.abandoned, "stop": result.stop,
    })
}

struct Session {
    start: Position,
    game: Option<Game>,
    local: bool,
    revision_base: usize,
    epoch: Instant,
    table_path: Option<PathBuf>,
    table: Option<Tablebase>,
}

impl Session {
    fn time(&self) -> (u64, u64) {
        (
            self.epoch.elapsed().as_millis() as u64,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        )
    }

    fn game(&mut self) -> Result<&mut Game, String> {
        self.game
            .as_mut()
            .ok_or_else(|| "session has no game".into())
    }

    fn update_clock(&mut self) -> Result<(), String> {
        let (now, stamp) = self.time();
        self.game()?.tick(now, stamp)
    }

    fn snapshot(&self) -> Result<Value, String> {
        let game = self.game.as_ref().ok_or("session has no game")?;
        let position = game.position();
        let turns = game.turns().collect::<Vec<_>>();
        let pdn = if self.local {
            Some(pdn::export(&pdn::completed(
                self.start,
                turns.iter().copied(),
                result_token(game.outcome()),
            )?))
        } else {
            None
        };
        Ok(json!({
            "fen": position.to_string(), "start": self.start.to_string(), "side": position.side(),
            "policy": game.policy(), "outcome": game.outcome(), "offer": game.offer(), "clock": game.clock(),
            "legal": game.legal().into_iter().map(view_turn).collect::<Vec<_>>(),
            "turns": turns.into_iter().map(view_turn).collect::<Vec<_>>(),
            "revision": self.revision_base + game.events().len(), "pdn": pdn, "quiet_plies": game.history().quiet_plies(),
        }))
    }

    fn exact(&mut self) -> Result<Option<&Tablebase>, String> {
        if self.table.is_none() {
            if let Some(path) = &self.table_path {
                self.table = Some(Tablebase::load(path)?);
            }
        }
        Ok(self.table.as_ref())
    }

    fn request(&mut self, request: Request) -> Result<Value, String> {
        let (now, stamp) = self.time();
        match request {
            Request::New {
                fen,
                policy,
                clock,
                local,
                opening,
            } => {
                let position = fen
                    .map(|text| text.parse())
                    .transpose()?
                    .unwrap_or_else(Position::start);
                if !local
                    && (position != Position::start()
                        || policy == Policy::Study
                        || opening.is_some())
                {
                    return Err("competitive rooms start from the standard position".into());
                }
                let mut start = position;
                if let Some(opening) = opening {
                    if opening.len() > 3 {
                        return Err("opening accepts at most three turns".into());
                    }
                    for text in opening {
                        start = start.play(crate::board::resolve_turn(start, &text)?)?;
                    }
                }
                let next_game = Game::new(start, policy, clock, local, now)?;
                if let Some(game) = &self.game {
                    self.revision_base += game.events().len() + 1;
                }
                self.game = Some(next_game);
                self.start = start;
                self.local = local;
                self.snapshot()
            }
            Request::Restore { text, policy } if self.local => {
                let record = pdn::import(&text, false).map_err(|e| e.to_string())?;
                let mut game = Game::new(record.start, policy, None, true, now)?;
                for ply in record.line.plies {
                    game.apply(
                        game.position().side(),
                        Command::Move {
                            turn: ply.turn.to_string(),
                        },
                        now,
                        stamp,
                    )?;
                }
                if record.result != result_token(game.outcome()) {
                    return Err("saved result is not reproducible from these turns".into());
                }
                if let Some(previous) = &self.game {
                    self.revision_base += previous.events().len() + 1;
                }
                self.start = record.start;
                self.game = Some(game);
                self.snapshot()
            }
            Request::State => {
                self.update_clock()?;
                self.snapshot()
            }
            Request::Act {
                actor,
                revision,
                command,
            } => {
                let base = self.revision_base;
                let game = self.game()?;
                game.tick(now, stamp)?;
                if revision != base + game.events().len() {
                    return Err("stale revision. Refresh game state.".into());
                }
                game.apply(actor, command, now, stamp)?;
                self.snapshot()
            }
            Request::Bot {
                revision,
                millis,
                nodes,
                depth,
            } if self.local => {
                self.update_clock()?;
                let base = self.revision_base;
                let game = self.game()?;
                if revision != base + game.events().len() || game.legal().is_empty() {
                    return Err("no active bot turn at that revision".into());
                }
                let side = game.position().side();
                let remaining = game.clock().map_or(2000, |clock| {
                    if side == Side::Black {
                        clock.black_ms
                    } else {
                        clock.white_ms
                    }
                });
                let result = run_search(
                    game.history(),
                    millis.min(remaining.saturating_sub(12)),
                    nodes,
                    depth,
                    search::Rules::House,
                );
                let best = result
                    .best
                    .or_else(|| game.legal().first().copied())
                    .ok_or("no legal bot turn")?;
                let (now, stamp) = self.time();
                self.game()?.apply(
                    side,
                    Command::Move {
                        turn: best.to_string(),
                    },
                    now,
                    stamp,
                )?;
                Ok(json!({"state": self.snapshot()?, "search": result_view(&result)}))
            }
            Request::Analyze {
                fen,
                pdn: replay,
                millis,
                nodes,
                depth,
            } if self.local => {
                let explicit = match (fen, replay) {
                    (Some(fen), None) => Some(crate::game::History::new(fen.parse()?)),
                    (None, Some(text)) => {
                        let record = pdn::import(&text, false).map_err(|e| e.to_string())?;
                        let mut history = crate::game::History::new(record.start);
                        for ply in &record.line.plies {
                            history.push_generated(ply.turn, ply.after);
                        }
                        Some(history)
                    }
                    (None, None) => None,
                    _ => return Err("choose either FEN or replay history".into()),
                };
                let history = match &explicit {
                    Some(history) => history,
                    None => self.game()?.history(),
                };
                Ok(result_view(&run_search(
                    history,
                    millis,
                    nodes,
                    depth,
                    search::Rules::House,
                )))
            }
            Request::Import { text, partial } if self.local => {
                let record = pdn::import(&text, partial).map_err(|e| e.to_string())?;
                Ok(
                    json!({"start": record.start.to_string(), "sha256": record.sha256, "tags": record.tags,
                    "line": line_view(&record.line), "result": record.result, "diagnostic": record.diagnostic,
                    "pdn": pdn::export(&record)}),
                )
            }
            Request::Review { text, nodes } if self.local => {
                let record = pdn::import(&text, false).map_err(|e| e.to_string())?;
                serde_json::to_value(review::review(&record, self.exact()?, nodes)?)
                    .map_err(|e| e.to_string())
            }
            Request::Witness { fen, max } if self.local => {
                let table = self
                    .exact()?
                    .ok_or("configure a tablebase before requesting a witness")?;
                serde_json::to_value(review::witness(fen.parse()?, table, max)?)
                    .map_err(|e| e.to_string())
            }
            _ => Err("command unavailable for this session".into()),
        }
    }
}

fn run_search(
    history: &crate::game::History,
    millis: u64,
    nodes: u64,
    depth: u8,
    rules: search::Rules,
) -> search::SearchResult {
    let now = Instant::now();
    let hard = Duration::from_millis(millis.min(2000));
    search::analyze(
        history,
        &Classical,
        search::Limits {
            depth,
            nodes: nodes.min(2_000_000),
            soft: Some(now + hard.mul_f64(0.85)),
            hard: Some(now + hard),
            cancel: None,
        },
        search::Config {
            rules,
            ..search::Config::default()
        },
    )
}

pub fn serve(table_path: Option<PathBuf>) -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut session = Session {
        start: Position::start(),
        game: None,
        local: true,
        revision_base: 0,
        epoch: Instant::now(),
        table_path,
        table: None,
    };
    loop {
        let mut header = [0u8; 8];
        match input.read(&mut header[..1])? {
            0 => return Ok(()),
            1 => input.read_exact(&mut header[1..])?,
            _ => unreachable!(),
        }
        if header[..4] != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "protocol version mismatch",
            ));
        }
        let size = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        if size == 0 || size > INPUT_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request frame out of range",
            ));
        }
        let mut body = vec![0; size];
        input.read_exact(&mut body)?;
        let result = serde_json::from_slice::<Request>(&body)
            .map_err(|e| e.to_string())
            .and_then(|request| session.request(request));
        let envelope = match result {
            Ok(value) => json!({"ok": true, "value": value}),
            Err(error) => json!({"ok": false, "error": error}),
        };
        let mut encoded = serde_json::to_vec(&envelope)?;
        if encoded.len() > OUTPUT_MAX {
            encoded = br#"{"ok":false,"error":"response exceeds protocol cap"}"#.to_vec();
        }
        header[4..].copy_from_slice(&(encoded.len() as u32).to_le_bytes());
        output.write_all(&header)?;
        output.write_all(&encoded)?;
        output.flush()?;
    }
}
