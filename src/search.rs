use crate::{
    board::{Position, Turn},
    eval::Evaluator,
    game::{History, board_outcome},
    moves,
    tablebase::{Tablebase, Wdl},
};
use serde::Serialize;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

pub const MATE: i32 = 30_000;
const PROOF_WIN: i32 = 25_000;
const INF: i32 = 31_000;
const MAX_DEPTH: u8 = 32;
const MAX_PLY: usize = MAX_DEPTH as usize + 25;

#[derive(Clone, Copy)]
pub struct Limits<'a> {
    pub depth: u8,
    pub nodes: u64,
    pub soft: Option<Instant>,
    pub hard: Option<Instant>,
    pub cancel: Option<&'a AtomicBool>,
}

impl Limits<'_> {
    pub fn fixed(depth: u8, nodes: u64) -> Self {
        Self {
            depth,
            nodes,
            soft: None,
            hard: None,
            cancel: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rules {
    House,
    Board,
}

#[derive(Clone, Copy)]
pub struct Config {
    pub table_entries: usize,
    pub selective: bool,
    pub rules: Rules,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            table_entries: 1 << 15,
            selective: false,
            rules: Rules::House,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Depth {
    pub depth: u8,
    pub score: i32,
    pub nodes: u64,
}

#[derive(Debug)]
pub struct SearchResult {
    pub best: Option<Turn>,
    pub pv: Vec<Turn>,
    pub score: Option<i32>,
    pub depths: Vec<Depth>,
    pub nodes: u64,
    pub cutoffs: u64,
    pub table_hits: u64,
    pub proof_hits: u64,
    pub table_bytes: usize,
    pub abandoned: Option<u8>,
    pub stop: &'static str,
}

#[derive(Clone, Copy)]
enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy)]
struct Entry {
    position: Position,
    depth: u8,
    ply: u8,
    score: i32,
    bound: Bound,
    best: Turn,
}

#[derive(Clone, Copy)]
struct Window {
    alpha: i32,
    beta: i32,
}

impl Window {
    fn child(self) -> Self {
        Self {
            alpha: -self.beta,
            beta: -self.alpha,
        }
    }

    fn probe(self) -> Self {
        Self {
            alpha: -self.alpha - 1,
            beta: -self.alpha,
        }
    }
}

struct Frame {
    moves: Vec<Turn>,
    pv: [Option<Turn>; MAX_PLY],
}

struct Search<'a> {
    evaluator: &'a dyn Evaluator,
    limits: Limits<'a>,
    config: Config,
    table: Vec<Option<Entry>>,
    history_order: [[i32; 1024]; 2],
    nodes: u64,
    cutoffs: u64,
    hits: u64,
    proofs: u64,
    tablebase: Option<&'a Tablebase>,
}

type Interrupted = &'static str;

fn terminal_score(position: Position, turns: &[Turn], ply: usize) -> Option<i32> {
    board_outcome(position, turns).map(|result| {
        if result.winner == Some(position.side()) {
            MATE - ply as i32
        } else {
            -MATE + ply as i32
        }
    })
}

impl Search<'_> {
    fn check(&self) -> Result<(), Interrupted> {
        if self
            .limits
            .cancel
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            return Err("cancelled");
        }
        if self.nodes >= self.limits.nodes {
            return Err("node_limit");
        }
        if self.limits.hard.is_some_and(|end| Instant::now() >= end) {
            return Err("hard_deadline");
        }
        Ok(())
    }

    fn index(&self, position: Position) -> usize {
        position.key() as usize % self.table.len()
    }

    fn ordering(&self, position: Position, turn: Turn, suggested: Option<Turn>) -> i32 {
        if suggested == Some(turn) {
            return 1_000_000;
        }
        let i = usize::from(turn.from()) * 32 + usize::from(turn.to());
        let captured = turn.captured();
        let crown = position.side().crown(turn.to()) && position.kings() & (1 << turn.from()) == 0;
        20_000 * captured.count_ones() as i32
            + 3000 * (captured & position.kings()).count_ones() as i32
            + 2000 * i32::from(crown)
            + self.history_order[position.side() as usize][i]
    }

    fn node(
        &mut self,
        history: &mut History,
        depth: u8,
        ply: usize,
        mut window: Window,
        stack: &mut [Frame],
    ) -> Result<i32, Interrupted> {
        self.check()?;
        self.nodes += 1;
        let position = history.current();
        let (frame, deeper) = stack.split_first_mut().expect("bounded search stack");
        frame.pv.fill(None);
        moves::generate(position, &mut frame.moves);
        if let Some(score) = terminal_score(position, &frame.moves, ply) {
            return Ok(score);
        }
        if self.config.rules == Rules::House && history.house_draw().is_some() {
            return Ok(0);
        }
        if ply > 0 {
            if let Some(proof) = self.tablebase.and_then(|table| table.probe(position)) {
                self.proofs += 1;
                return Ok(proof_score(proof.wdl));
            }
        }
        if depth == 0 && !frame.moves[0].is_capture() {
            return Ok(self.evaluator.evaluate(position).clamp(-20_000, 20_000));
        }
        // A reversible suffix can affect future draw rights, so only fresh
        // history (or board-only search) may use a transposition bound.
        let index = (!self.table.is_empty()
            && depth > 0
            && (history.quiet_plies() == 0 || self.config.rules == Rules::Board))
            .then(|| self.index(position));
        let entry = index
            .and_then(|i| self.table[i])
            .filter(|e| e.position == position);
        if let Some(entry) =
            entry.filter(|e| ply > 0 && e.depth == depth && usize::from(e.ply) == ply)
        {
            let usable = match entry.bound {
                Bound::Exact => true,
                Bound::Lower => entry.score >= window.beta,
                Bound::Upper => entry.score <= window.alpha,
            };
            if usable {
                self.hits += 1;
                frame.pv[0] = Some(entry.best);
                return Ok(entry.score);
            }
        }
        let suggested = entry.map(|entry| entry.best);
        frame
            .moves
            .sort_by_cached_key(|&turn| -self.ordering(position, turn, suggested));
        let original_alpha = window.alpha;
        let mut best = -INF;
        let mut best_turn = frame.moves[0];
        for (index, &turn) in frame.moves.iter().enumerate() {
            let mut next = position;
            next.make(turn);
            history.push_generated(turn, next);
            let child_depth = depth.saturating_sub(1);
            let attempt = (|| {
                if index == 0 {
                    return self
                        .node(history, child_depth, ply + 1, window.child(), deeper)
                        .map(|s| -s);
                }
                let reduced = self.config.selective
                    && index >= 4
                    && depth >= 4
                    && !turn.is_capture()
                    && (window.beta - window.alpha <= 1)
                    && !position.side().crown(turn.to());
                if reduced {
                    let quick =
                        -self.node(history, child_depth - 1, ply + 1, window.probe(), deeper)?;
                    if quick <= window.alpha {
                        return Ok(quick);
                    }
                }
                let score = -self.node(history, child_depth, ply + 1, window.probe(), deeper)?;
                if score > window.alpha && score < window.beta {
                    return self
                        .node(history, child_depth, ply + 1, window.child(), deeper)
                        .map(|s| -s);
                }
                Ok(score)
            })();
            history.pop();
            let score = attempt?;
            if score > best {
                best = score;
                best_turn = turn;
                frame.pv[0] = Some(turn);
                frame.pv[1..].copy_from_slice(&deeper[0].pv[..MAX_PLY - 1]);
            }
            window.alpha = window.alpha.max(score);
            if window.alpha >= window.beta {
                self.cutoffs += 1;
                if !turn.is_capture() {
                    let slot = usize::from(turn.from()) * 32 + usize::from(turn.to());
                    let bonus = i32::from(depth).pow(2);
                    let previous = &mut self.history_order[position.side() as usize][slot];
                    *previous = (*previous + bonus).min(1500);
                }
                break;
            }
        }
        if let Some(index) = index {
            let bound = if best <= original_alpha {
                Bound::Upper
            } else if best >= window.beta {
                Bound::Lower
            } else {
                Bound::Exact
            };
            if self.table[index]
                .is_none_or(|entry| entry.position == position || entry.depth <= depth)
            {
                self.table[index] = Some(Entry {
                    position,
                    depth,
                    ply: ply as u8,
                    score: best,
                    bound,
                    best: best_turn,
                });
            }
        }
        Ok(best)
    }
}

fn proof_score(value: Wdl) -> i32 {
    match value {
        Wdl::Win => PROOF_WIN,
        Wdl::Loss => -PROOF_WIN,
        Wdl::Draw => 0,
    }
}

pub fn analyze(
    history: &History,
    evaluator: &dyn Evaluator,
    limits: Limits<'_>,
    config: Config,
) -> SearchResult {
    analyze_with_table(history, evaluator, limits, config, None)
}

pub fn analyze_with_table(
    history: &History,
    evaluator: &dyn Evaluator,
    limits: Limits<'_>,
    config: Config,
    tablebase: Option<&Tablebase>,
) -> SearchResult {
    let tablebase = if config.rules == Rules::Board {
        tablebase
    } else {
        None
    };
    let table_entries = config.table_entries.min(1 << 20);
    let mut engine = Search {
        evaluator,
        limits,
        config,
        table: vec![None; table_entries],
        history_order: [[0; 1024]; 2],
        nodes: 0,
        cutoffs: 0,
        hits: 0,
        proofs: 0,
        tablebase,
    };
    let position = history.current();
    let root = moves::legal(position);
    let terminal = terminal_score(position, &root, 0)
        .or_else(|| (config.rules == Rules::House && history.house_draw().is_some()).then_some(0));
    let mut result = SearchResult {
        best: if terminal.is_none() {
            root.first().copied()
        } else {
            None
        },
        pv: Vec::new(),
        score: terminal,
        depths: Vec::new(),
        nodes: 0,
        cutoffs: 0,
        table_hits: 0,
        proof_hits: 0,
        table_bytes: table_entries * size_of::<Option<Entry>>(),
        abandoned: None,
        stop: "depth_limit",
    };
    if terminal.is_some() {
        result.stop = "terminal";
        return result;
    }
    if let Some((table, proof)) =
        tablebase.and_then(|table| table.probe(position).map(|proof| (table, proof)))
    {
        let witness = root.iter().find(|&&turn| {
            let mut next = position;
            next.make(turn);
            table.probe(next).is_some_and(|child| {
                proof_score(child.wdl) == -proof_score(proof.wdl)
                    && (proof.wdl == Wdl::Draw || child.witness_rank < proof.witness_rank)
            })
        });
        if let Some(&turn) = witness {
            result.best = Some(turn);
            result.pv.push(turn);
            result.score = Some(proof_score(proof.wdl));
            result.proof_hits = 1;
            result.stop = "board_proof";
            return result;
        }
    }
    let mut history = history.clone();
    let mut stack = (0..MAX_PLY)
        .map(|_| Frame {
            moves: Vec::with_capacity(48),
            pv: [None; MAX_PLY],
        })
        .collect::<Vec<_>>();
    for depth in 1..=limits.depth.clamp(1, MAX_DEPTH) {
        if limits.soft.is_some_and(|end| Instant::now() >= end) {
            result.stop = "soft_deadline";
            break;
        }
        let window = result.score.filter(|s| s.abs() < MATE - 100).map_or(
            Window {
                alpha: -INF,
                beta: INF,
            },
            |s| Window {
                alpha: s - 40,
                beta: s + 40,
            },
        );
        let score = engine
            .node(&mut history, depth, 0, window, &mut stack)
            .and_then(|score| {
                if score <= window.alpha || score >= window.beta {
                    engine.node(
                        &mut history,
                        depth,
                        0,
                        Window {
                            alpha: -INF,
                            beta: INF,
                        },
                        &mut stack,
                    )
                } else {
                    Ok(score)
                }
            });
        match score {
            Ok(score) => {
                result.pv = stack[0].pv.iter().map_while(|&turn| turn).collect();
                result.best = result.pv.first().copied().or(result.best);
                result.score = Some(score);
                result.depths.push(Depth {
                    depth,
                    score,
                    nodes: engine.nodes,
                });
                if score.abs() >= MATE - 100 {
                    result.stop = "forced_terminal";
                    break;
                }
            }
            Err(stop) => {
                result.abandoned = Some(depth);
                result.stop = stop;
                break;
            }
        }
    }
    result.nodes = engine.nodes;
    result.cutoffs = engine.cutoffs;
    result.table_hits = engine.hits;
    result.proof_hits = engine.proofs;
    result
}

pub fn minimax(history: &History, evaluator: &dyn Evaluator, depth: u8, rules: Rules) -> i32 {
    fn visit(
        history: &mut History,
        evaluator: &dyn Evaluator,
        depth: u8,
        rules: Rules,
        ply: usize,
    ) -> i32 {
        let position = history.current();
        let mut turns = Vec::new();
        crate::oracle::generate(position, &mut turns);
        if let Some(score) = terminal_score(position, &turns, ply) {
            return score;
        }
        if rules == Rules::House && history.house_draw().is_some() {
            return 0;
        }
        if depth == 0 && !turns[0].is_capture() {
            return evaluator.evaluate(position).clamp(-20_000, 20_000);
        }
        turns
            .into_iter()
            .map(|turn| {
                history.push_generated(turn, crate::oracle::advance(position, turn));
                let score = -visit(history, evaluator, depth.saturating_sub(1), rules, ply + 1);
                history.pop();
                score
            })
            .max()
            .unwrap()
    }
    visit(&mut history.clone(), evaluator, depth.min(6), rules, 0)
}
