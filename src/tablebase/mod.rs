pub mod frontier;
mod rank;
mod retrograde;
mod verify;

pub use rank::{rank, slots, unrank};
pub use retrograde::build;
pub use verify::verify;

use crate::board::Position;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
};

const SCHEMA: &str = "crownline-board-wdl-1";
const MAX_COUNT: u32 = 4;
const DISTANCE_MASK: u32 = 0x3fff_ffff;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Wdl {
    Win,
    Loss,
    Draw,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Proof {
    pub wdl: Wdl,
    pub witness_rank: Option<u32>,
}

impl Proof {
    fn decisive(wdl: Wdl, rank: u32) -> u32 {
        assert!(rank <= DISTANCE_MASK);
        (if wdl == Wdl::Win { 1 } else { 2 }) << 30 | rank
    }

    fn decode(word: u32) -> Option<Self> {
        let distance = word & DISTANCE_MASK;
        match word >> 30 {
            1 => Some(Self {
                wdl: Wdl::Win,
                witness_rank: Some(distance),
            }),
            2 => Some(Self {
                wdl: Wdl::Loss,
                witness_rank: Some(distance),
            }),
            3 if distance == 0 => Some(Self {
                wdl: Wdl::Draw,
                witness_rank: None,
            }),
            _ => None,
        }
    }
}

const DRAW: u32 = 3 << 30;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Shard {
    pub count: u32,
    pub slots: u32,
    pub win: u64,
    pub loss: u64,
    pub draw: u64,
    pub invalid: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub schema: String,
    pub shards: Vec<Shard>,
}

pub struct Tablebase {
    inventory: Inventory,
    slices: Vec<Vec<u32>>,
}

fn shard_path(dir: &Path, count: u32) -> std::path::PathBuf {
    dir.join(format!("pieces-{count}.bin"))
}

pub(crate) fn atomic(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<()> {
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = write(&mut file)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            fs::rename(&temp, path)?;
            fs::File::open(
                path.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )?
            .sync_all()
        });
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn shard_stats(count: u32, entries: &[u32], sha256: String) -> Result<Shard, String> {
    let mut result = Shard {
        count,
        slots: slots(count),
        win: 0,
        loss: 0,
        draw: 0,
        invalid: 0,
        sha256,
    };
    for &entry in entries {
        match Proof::decode(entry).map(|p| p.wdl) {
            Some(Wdl::Win) => result.win += 1,
            Some(Wdl::Loss) => result.loss += 1,
            Some(Wdl::Draw) => result.draw += 1,
            None if entry == 0 => result.invalid += 1,
            None => return Err("bad tablebase entry encoding".into()),
        }
    }
    Ok(result)
}

impl Tablebase {
    fn empty() -> Self {
        Self {
            inventory: Inventory {
                schema: SCHEMA.into(),
                shards: Vec::new(),
            },
            slices: Vec::new(),
        }
    }

    pub fn load(dir: &Path) -> Result<Self, String> {
        let manifest = fs::File::open(dir.join("inventory.json")).map_err(|e| e.to_string())?;
        if manifest.metadata().map_err(|e| e.to_string())?.len() > 16_384 {
            return Err("inventory exceeds maximum size".into());
        }
        let inventory: Inventory = serde_json::from_reader(manifest).map_err(|e| e.to_string())?;
        if inventory.schema != SCHEMA
            || inventory.shards.is_empty()
            || inventory.shards.len() > MAX_COUNT as usize
        {
            return Err("unrecognized or empty tablebase".into());
        }
        let mut slices = Vec::new();
        for (i, shard) in inventory.shards.iter().enumerate() {
            let count = i as u32 + 1;
            let bytes = u64::from(slots(count)) * 4;
            if shard.count != count || shard.slots != slots(count) {
                return Err(format!("wrong shard dimensions for {count} pieces"));
            }
            let file = fs::File::open(shard_path(dir, count)).map_err(|e| e.to_string())?;
            if file.metadata().map_err(|e| e.to_string())?.len() != bytes {
                return Err(format!("wrong shard length for {count} pieces"));
            }
            let mut raw = Vec::with_capacity(bytes as usize);
            file.take(bytes + 1)
                .read_to_end(&mut raw)
                .map_err(|e| e.to_string())?;
            let sha256 = format!("{:x}", Sha256::digest(&raw));
            if raw.len() as u64 != bytes || sha256 != shard.sha256 {
                return Err(format!("shard checksum mismatch for {count} pieces"));
            }
            let entries = raw
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect::<Vec<_>>();
            if shard_stats(count, &entries, sha256)? != *shard {
                return Err(format!("shard totals mismatch for {count} pieces"));
            }
            slices.push(entries);
        }
        Ok(Self { inventory, slices })
    }

    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    pub fn probe(&self, position: Position) -> Option<Proof> {
        if position.occupied().count_ones() > self.slices.len() as u32 {
            return None;
        }
        let (count, slot) = rank(position);
        self.lookup(count, slot)
    }

    fn lookup(&self, count: u32, slot: u32) -> Option<Proof> {
        Proof::decode(self.slices.get(count as usize - 1)?[slot as usize])
    }

    fn add(&mut self, dir: &Path, entries: Vec<u32>) -> Result<(), String> {
        let count = self.slices.len() as u32 + 1;
        let mut digest = Sha256::new();
        atomic(&shard_path(dir, count), |file| {
            let mut out = io::BufWriter::new(file);
            let mut block = [0u8; 16_384];
            for chunk in entries.chunks(block.len() / 4) {
                let raw = &mut block[..chunk.len() * 4];
                for (dst, value) in raw.chunks_exact_mut(4).zip(chunk) {
                    dst.copy_from_slice(&value.to_le_bytes());
                }
                digest.update(&*raw);
                out.write_all(raw)?;
            }
            out.flush()
        })
        .map_err(|e| e.to_string())?;
        let stats = shard_stats(count, &entries, format!("{:x}", digest.finalize()))?;
        self.inventory.shards.push(stats);
        atomic(&dir.join("inventory.json"), |file| {
            serde_json::to_writer_pretty(file, &self.inventory).map_err(io::Error::other)
        })
        .map_err(|e| e.to_string())?;
        self.slices.push(entries);
        Ok(())
    }
}
