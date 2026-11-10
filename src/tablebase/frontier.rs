use super::{
    Tablebase, atomic,
    rank::{canonical, rank},
};
use crate::{
    board::{Position, Turn},
    moves, oracle,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io, path::Path};

const MAX_EXPANDED: usize = 40_000;
const MAX_DISCOVERED: usize = 350_000;
const SCHEMA: &str = "crownline-forward-partition-1";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Partition {
    schema: String,
    start: String,
    pub expanded: usize,
    pub closed_routes: u64,
    pub internal_edges: u64,
    pub discovered: Vec<String>,
}

#[derive(Serialize)]
pub struct Frontier {
    pub expanded: usize,
    pub discovered: usize,
    pub unresolved: usize,
    pub closed_routes: u64,
    pub internal_edges: u64,
}

fn successors(
    position: Position,
    oracle_check: bool,
    legal: &mut Vec<Turn>,
    next: &mut Vec<Position>,
) {
    next.clear();
    if oracle_check {
        oracle::generate(position, legal);
    } else {
        moves::generate(position, legal);
    }
    for &turn in legal.iter() {
        let child = if oracle_check {
            oracle::advance(position, turn)
        } else {
            let mut child = position;
            child.make(turn);
            child
        };
        next.push(canonical(child));
    }
    next.sort_by_cached_key(|&position| rank(position));
}

fn expand(
    start: Position,
    seed: Option<Partition>,
    stop: usize,
    table: &Tablebase,
    oracle_check: bool,
) -> Result<Partition, String> {
    let start = canonical(start);
    let count = start.occupied().count_ones();
    if !(5..=6).contains(&count)
        || stop == 0
        || stop > MAX_EXPANDED
        || table.inventory().shards.len() != 4
    {
        return Err("frontier requires a five/six-piece start, a four-piece table, and a limit from 1 through 40000".into());
    }
    let mut partition = seed.unwrap_or_else(|| Partition {
        schema: SCHEMA.into(),
        start: start.to_string(),
        expanded: 0,
        closed_routes: 0,
        internal_edges: 0,
        discovered: vec![start.to_string()],
    });
    if partition.schema != SCHEMA
        || partition.start != start.to_string()
        || partition.expanded > stop
        || partition.expanded > partition.discovered.len()
        || partition.discovered.len() > MAX_DISCOVERED
        || partition.discovered.first() != Some(&partition.start)
    {
        return Err("partition metadata conflicts with requested resume".into());
    }
    let mut queue = partition
        .discovered
        .iter()
        .map(|fen| fen.parse::<Position>())
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = queue
        .iter()
        .map(|&position| rank(position))
        .collect::<HashSet<_>>();
    if seen.len() != queue.len()
        || queue
            .iter()
            .any(|&p| p != canonical(p) || p.occupied().count_ones() <= 4)
    {
        return Err("partition contains duplicate, noncanonical or out-of-scope states".into());
    }
    let mut turns = Vec::new();
    let mut next = Vec::new();
    while partition.expanded < stop && partition.expanded < queue.len() {
        successors(
            queue[partition.expanded],
            oracle_check,
            &mut turns,
            &mut next,
        );
        let mut previous = None;
        for &child in &next {
            if child.occupied().count_ones() <= 4 {
                if table.probe(child).is_none() {
                    return Err("missing closed-slice exit".into());
                }
                partition.closed_routes += 1;
                continue;
            }
            let id = rank(child);
            if previous == Some(id) {
                continue;
            }
            previous = Some(id);
            partition.internal_edges += 1;
            if seen.insert(id) {
                if queue.len() >= MAX_DISCOVERED {
                    return Err("frontier reached the memory bound".into());
                }
                queue.push(child);
                partition.discovered.push(child.to_string());
            }
        }
        partition.expanded += 1;
    }
    Ok(partition)
}

fn read(path: &Path) -> Result<Partition, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > 24_000_000 {
        return Err("partition exceeds file cap".into());
    }
    serde_json::from_reader(file).map_err(|e| e.to_string())
}

pub fn build(
    path: &Path,
    start: Position,
    stop: usize,
    table: &Tablebase,
) -> Result<Frontier, String> {
    let seed = if path.exists() {
        let existing = read(path)?;
        compare(&existing, table)?;
        Some(existing)
    } else {
        None
    };
    let partition = expand(start, seed, stop, table, false)?;
    atomic(path, |file| {
        serde_json::to_writer(file, &partition).map_err(io::Error::other)
    })
    .map_err(|e| e.to_string())?;
    Ok(summary(&partition))
}

fn summary(partition: &Partition) -> Frontier {
    Frontier {
        expanded: partition.expanded,
        discovered: partition.discovered.len(),
        unresolved: partition.discovered.len() - partition.expanded,
        closed_routes: partition.closed_routes,
        internal_edges: partition.internal_edges,
    }
}

fn compare(recorded: &Partition, table: &Tablebase) -> Result<Frontier, String> {
    if recorded.schema != SCHEMA {
        return Err("unsupported partition schema".into());
    }
    let actual = expand(
        recorded.start.parse()?,
        None,
        recorded.expanded,
        table,
        true,
    )?;
    if actual.discovered != recorded.discovered
        || actual.closed_routes != recorded.closed_routes
        || actual.internal_edges != recorded.internal_edges
        || actual.expanded != recorded.expanded
    {
        return Err("partition does not match independently replayed frontier".into());
    }
    Ok(summary(&actual))
}

pub fn verify(path: &Path, table: &Tablebase) -> Result<Frontier, String> {
    compare(&read(path)?, table)
}
