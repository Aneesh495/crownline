use super::{
    Tablebase, Wdl,
    rank::{rank, slots, unrank},
};
use crate::{board::Side, oracle};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Verification {
    pub slots: u64,
    pub positions: u64,
    pub routes: u64,
    pub cross_slice_routes: u64,
}

pub fn verify(table: &Tablebase) -> Result<Verification, String> {
    let mut tally = Verification {
        slots: 0,
        positions: 0,
        routes: 0,
        cross_slice_routes: 0,
    };
    let mut routes = Vec::with_capacity(32);
    for shard in &table.inventory.shards {
        for slot in 0..slots(shard.count) {
            tally.slots += 1;
            let proof = table.lookup(shard.count, slot);
            let Some(position) = unrank(shard.count, slot) else {
                if proof.is_some() {
                    return Err(format!(
                        "proof assigned to illegal slot {}:{slot}",
                        shard.count
                    ));
                }
                continue;
            };
            if rank(position) != (shard.count, slot)
                || rank(position.flipped()) != (shard.count, slot)
            {
                return Err(format!(
                    "ranking bijection broken at {}:{slot}",
                    shard.count
                ));
            }
            let proof = proof.ok_or_else(|| format!("missing proof at {position}"))?;
            oracle::generate(position, &mut routes);
            tally.positions += 1;
            tally.routes += routes.len() as u64;
            if routes.is_empty() {
                let expected = if position.mask(Side::White) == 0 {
                    Wdl::Win
                } else {
                    Wdl::Loss
                };
                if proof.wdl != expected || proof.witness_rank != Some(0) {
                    return Err(format!("incorrect terminal proof at {position}"));
                }
                continue;
            }
            let mut any_loss = false;
            let mut any_draw = false;
            let mut all_win = true;
            let mut decreasing_loss = false;
            let mut all_decreasing_wins = true;
            for &turn in &routes {
                let child = oracle::advance(position, turn);
                tally.cross_slice_routes += u64::from(child.occupied().count_ones() < shard.count);
                let next = table
                    .probe(child)
                    .ok_or_else(|| format!("open frontier {position} {turn} {child}"))?;
                any_loss |= next.wdl == Wdl::Loss;
                any_draw |= next.wdl == Wdl::Draw;
                all_win &= next.wdl == Wdl::Win;
                let decreases = next
                    .witness_rank
                    .zip(proof.witness_rank)
                    .is_some_and(|(child, parent)| child < parent);
                decreasing_loss |= next.wdl == Wdl::Loss && decreases;
                all_decreasing_wins &= next.wdl == Wdl::Win && decreases;
            }
            let valid = match proof.wdl {
                Wdl::Win => decreasing_loss,
                Wdl::Loss => all_win && all_decreasing_wins,
                Wdl::Draw => !any_loss && any_draw,
            };
            if !valid {
                return Err(format!(
                    "WDL recurrence or finite witness violated at {position} ({:?})",
                    proof.wdl
                ));
            }
        }
    }
    Ok(tally)
}
