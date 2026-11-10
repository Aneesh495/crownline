use super::{
    DRAW, MAX_COUNT, Proof, Tablebase, Wdl,
    rank::{entries, rank, slots},
};
use crate::{board::Position, moves};
use std::{collections::VecDeque, fs, path::Path};

fn successors(
    position: Position,
    routes: &mut Vec<crate::board::Turn>,
    output: &mut Vec<(u32, u32)>,
) {
    moves::generate(position, routes);
    output.clear();
    for &turn in routes.iter() {
        let mut next = position;
        next.make(turn);
        output.push(rank(next));
    }
    output.sort_unstable();
    output.dedup();
}

fn slice(previous: &Tablebase, count: u32) -> Result<Vec<u32>, String> {
    let size = slots(count) as usize;
    let mut values = vec![0u32; size];
    let mut legal = vec![false; size];
    let mut remaining = vec![0u8; size];
    let mut rank_max = vec![0u32; size];
    let mut offsets = vec![0u32; size + 1];
    let mut queue = VecDeque::new();
    let mut routes = Vec::with_capacity(32);
    let mut edges = Vec::with_capacity(32);

    for (source, position) in entries(count) {
        let Some(position) = position else { continue };
        let i = source as usize;
        legal[i] = true;
        successors(position, &mut routes, &mut edges);
        if edges.is_empty() {
            let win = position.elimination() == Some(position.side());
            values[i] = Proof::decisive(if win { Wdl::Win } else { Wdl::Loss }, 0);
            queue.push_back(source);
            continue;
        }
        let mut winning_witness = None;
        for &(child_count, child) in &edges {
            if child_count == count {
                remaining[i] += 1;
                offsets[child as usize + 1] += 1;
            } else {
                let proof = previous
                    .lookup(child_count, child)
                    .ok_or("unknown lower-slice successor")?;
                match proof.wdl {
                    Wdl::Loss => winning_witness = proof.witness_rank.map(|rank| rank + 1),
                    Wdl::Win => rank_max[i] = rank_max[i].max(proof.witness_rank.unwrap()),
                    Wdl::Draw => remaining[i] += 1,
                }
            }
        }
        if let Some(witness) = winning_witness {
            values[i] = Proof::decisive(Wdl::Win, witness);
            queue.push_back(source);
        } else if remaining[i] == 0 {
            values[i] = Proof::decisive(Wdl::Loss, rank_max[i] + 1);
            queue.push_back(source);
        }
    }
    for i in 1..offsets.len() {
        offsets[i] = offsets[i]
            .checked_add(offsets[i - 1])
            .ok_or("too many reverse edges")?;
    }
    let mut reverse = vec![0u32; offsets[size] as usize];
    let mut cursor = offsets[..size].to_vec();
    for (source, position) in entries(count) {
        let Some(position) = position else { continue };
        if values[source as usize] != 0 {
            continue;
        }
        successors(position, &mut routes, &mut edges);
        for &(child_count, child) in &edges {
            if child_count == count {
                let next = &mut cursor[child as usize];
                reverse[*next as usize] = source;
                *next += 1;
            }
        }
    }

    while let Some(child) = queue.pop_front() {
        let proof = Proof::decode(values[child as usize]).unwrap();
        let witness = proof.witness_rank.unwrap();
        for &source in &reverse[offsets[child as usize] as usize..cursor[child as usize] as usize] {
            let i = source as usize;
            if values[i] != 0 {
                continue;
            }
            match proof.wdl {
                Wdl::Loss => values[i] = Proof::decisive(Wdl::Win, witness + 1),
                Wdl::Win => {
                    remaining[i] -= 1;
                    rank_max[i] = rank_max[i].max(witness);
                    if remaining[i] == 0 {
                        values[i] = Proof::decisive(Wdl::Loss, rank_max[i] + 1);
                    }
                }
                Wdl::Draw => unreachable!(),
            }
            if values[i] != 0 {
                queue.push_back(source);
            }
        }
    }
    for (value, valid) in values.iter_mut().zip(legal) {
        if *value == 0 && valid {
            *value = DRAW;
        }
    }
    Ok(values)
}

pub fn build(dir: &Path, max: u32) -> Result<Tablebase, String> {
    if !(1..=MAX_COUNT).contains(&max) {
        return Err(format!(
            "closed solver supports 1 through {MAX_COUNT} pieces"
        ));
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let lock = dir.join(".build-lock");
    let _lock = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock)
        .map_err(|e| format!("solver lock {}: {e}", lock.display()))?;
    let result = (|| {
        let mut tables = if dir.join("inventory.json").exists() {
            Tablebase::load(dir)?
        } else {
            Tablebase::empty()
        };
        if tables.slices.len() as u32 > max {
            return Err("checkpoint already exceeds requested piece count".into());
        }
        for count in (tables.slices.len() as u32 + 1)..=max {
            eprintln!("building {count}-piece shard");
            let results = slice(&tables, count)?;
            tables.add(dir, results)?;
            eprintln!(
                "{}",
                serde_json::to_string(tables.inventory.shards.last().unwrap()).unwrap()
            );
        }
        Ok(tables)
    })();
    fs::remove_file(lock).map_err(|e| e.to_string())?;
    result
}
