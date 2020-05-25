use super::{
    Ending, Finish, GAME_LIMIT, Opening, Split, append,
    arena::{config, scheduled},
    file_hash, play,
};
use crate::{
    board::{Position, Side, Turn, coordinates, squares},
    eval::Classical,
    search::{Limits, analyze},
    tablebase::atomic,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const MARCHER_COMMIT: &str = "1fa785edbe8003445163a63f00f04a4c0b77254b";
pub const MICROS: u64 = 4000;

struct Marcher {
    process: Child,
    input: ChildStdin,
    receiver: Receiver<String>,
    reader: Option<JoinHandle<()>>,
}

impl Marcher {
    fn launch(executable: &Path) -> Result<Self, String> {
        let mut process = Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let input = process.stdin.take().unwrap();
        let output = process.stdout.take().unwrap();
        let (sender, receiver) = mpsc::sync_channel(2);
        let reader = thread::spawn(move || {
            let mut input = BufReader::new(output);
            loop {
                let mut bytes = Vec::with_capacity(80);
                match input.by_ref().take(80).read_until(b'\n', &mut bytes) {
                    Ok(0 | 80) | Err(_) => break,
                    Ok(_) => {}
                }
                let Ok(line) = String::from_utf8(bytes) else {
                    break;
                };
                if sender.try_send(line).is_err() {
                    break;
                }
            }
        });
        let comparator = Self {
            process,
            input,
            receiver,
            reader: Some(reader),
        };
        let ready = comparator
            .receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| format!("comparator startup handshake: {error}"))?;
        if ready.trim_end() != "ready" {
            return Err("invalid comparator startup handshake".into());
        }
        Ok(comparator)
    }

    fn step(
        &mut self,
        masks: [u64; 4],
        player: usize,
        forced: i8,
        start: Instant,
    ) -> Result<(u8, u8, u64), Finish> {
        let elapsed = start.elapsed().as_micros() as u64;
        let micros = MICROS.saturating_sub(elapsed).max(25);
        writeln!(
            self.input,
            "{:x} {:x} {:x} {:x} {player} {forced} {micros}",
            masks[0], masks[1], masks[2], masks[3]
        )
        .and_then(|()| self.input.flush())
        .map_err(|_| Finish::Crash)?;
        let remaining = Duration::from_millis(75).saturating_sub(start.elapsed());
        let response = self.receiver.recv_timeout(remaining).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => Finish::Timeout,
            mpsc::RecvTimeoutError::Disconnected => Finish::Crash,
        })?;
        let mut fields = response.split_whitespace();
        let mut square = || {
            let raw = fields.next()?.parse::<u8>().ok()?;
            let row = raw / 8;
            let col = raw % 8;
            (row < 8 && (row + col) % 2 == 1).then_some(row * 4 + col / 2)
        };
        let from = square().ok_or(Finish::Illegal)?;
        let to = square().ok_or(Finish::Illegal)?;
        let nodes = fields
            .next()
            .and_then(|nodes| nodes.parse().ok())
            .ok_or(Finish::Crash)?;
        if fields.next().is_some() {
            return Err(Finish::Crash);
        }
        Ok((from, to, nodes))
    }

    fn turn(
        &mut self,
        position: Position,
        legal: &[Turn],
        start: Instant,
    ) -> Result<(Turn, u64), Finish> {
        let mut masks = [0u64; 4];
        for side in [Side::White, Side::Black] {
            let slot = usize::from(side == Side::Black);
            for square in squares(position.mask(side)) {
                let king = usize::from(position.kings() & (1 << square) != 0);
                masks[slot + 2 * king] |= 1 << square64(square);
            }
        }
        let player = if position.side() == Side::White { 1 } else { 2 };
        let mut candidates = legal.to_vec();
        let mut forced = -1;
        let mut visited = 0;
        for index in 0..12 {
            let (from, to, nodes) = self.step(masks, player, forced, start)?;
            visited += nodes;
            candidates.retain(|turn| turn.path().get(index..=index + 1) == Some(&[from, to]));
            match candidates.as_slice() {
                [] => return Err(Finish::Illegal),
                [turn] => return Ok((*turn, visited)),
                _ => {}
            }
            let old = square64(from);
            let new = square64(to);
            let victim = (old + new) / 2;
            let slot = masks
                .iter()
                .position(|mask| mask & (1u64 << old) != 0)
                .ok_or(Finish::Illegal)?;
            masks[slot] ^= (1u64 << old) | (1u64 << new);
            for mask in &mut masks {
                *mask &= !(1u64 << victim);
            }
            forced = new as i8;
        }
        Err(Finish::Illegal)
    }
}

impl Drop for Marcher {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn square64(square: u8) -> u8 {
    let (row, col) = coordinates(square);
    (8 * row + col) as u8
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub comparator_commit: String,
    pub comparator_sha256: String,
    pub pairs: usize,
    pub search_micros: u64,
    pub ply_cap: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Game {
    pub pair: usize,
    pub crownline: Side,
    pub opening: Opening,
    pub moves: Vec<String>,
    pub ending: Ending,
    pub cumulative_nodes: [u64; 2],
    pub cumulative_micros: [u64; 2],
    pub overruns_2x: [usize; 2],
}

pub fn run(executable: &Path, directory: &Path, pairs: usize) -> Result<serde_json::Value, String> {
    let openings = scheduled(&Split::Test);
    if pairs == 0 || pairs > openings.len() {
        return Err("invalid reference pair count".into());
    }
    fs::create_dir(directory).map_err(|e| e.to_string())?;
    let manifest = Manifest {
        comparator_commit: MARCHER_COMMIT.into(),
        comparator_sha256: file_hash(executable)?,
        pairs,
        search_micros: MICROS,
        ply_cap: GAME_LIMIT,
    };
    atomic(&directory.join("reference.json"), |f| {
        serde_json::to_writer_pretty(f, &manifest).map_err(io::Error::other)
    })
    .map_err(|e| e.to_string())?;
    let ledger = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("matches.jsonl"))
        .map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(ledger);
    let mut technical = 0;
    for (pair, opening) in openings.iter().enumerate().take(pairs) {
        for crownline in Side::ALL {
            let mut marcher = Marcher::launch(executable)?;
            let mut cumulative_nodes = [0; 2];
            let mut cumulative_micros = [0; 2];
            let mut overruns_2x = [0; 2];
            let (moves, ending) = play(opening, GAME_LIMIT, |history, legal, _| {
                let start = Instant::now();
                let position = history.current();
                let side = position.side();
                let slot = side as usize;
                let response = if side == crownline {
                    let result = analyze(
                        history,
                        &Classical,
                        Limits {
                            depth: 26,
                            nodes: 500_000,
                            soft: Some(start + Duration::from_micros(MICROS * 8 / 10)),
                            hard: Some(start + Duration::from_micros(MICROS)),
                            cancel: None,
                        },
                        config(),
                    );
                    result
                        .best
                        .map(|turn| (turn, result.nodes))
                        .ok_or(Finish::Crash)
                } else {
                    marcher.turn(position, legal, start)
                };
                let elapsed = start.elapsed().as_micros() as u64;
                cumulative_micros[slot] += elapsed;
                overruns_2x[slot] += usize::from(elapsed > 2 * MICROS);
                let (turn, nodes) = response?;
                cumulative_nodes[slot] += nodes;
                Ok(turn)
            })?;
            technical += usize::from(matches!(
                ending.reason,
                Finish::Illegal | Finish::Crash | Finish::Timeout
            ));
            append(
                &mut writer,
                &Game {
                    pair,
                    crownline,
                    opening: opening.clone(),
                    moves,
                    ending,
                    cumulative_nodes,
                    cumulative_micros,
                    overruns_2x,
                },
            )?;
        }
        writer.flush().map_err(|e| e.to_string())?;
        if (pair + 1) % 8 == 0 {
            eprintln!("reference pairs complete: {}", pair + 1);
        }
    }
    writer.get_ref().sync_all().map_err(|e| e.to_string())?;
    Ok(
        serde_json::json!({"games": 2 * pairs, "technical": technical, "comparator": manifest.comparator_commit}),
    )
}
