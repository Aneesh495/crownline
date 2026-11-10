use crate::board::{Position, Side, bit, squares};

const fn binomials() -> [[u32; 7]; 33] {
    let mut table = [[0; 7]; 33];
    let mut n = 0;
    while n <= 32 {
        table[n][0] = 1;
        let mut k = 1;
        while k <= 6 && k <= n {
            table[n][k] = table[n - 1][k - 1] + table[n - 1][k];
            k += 1;
        }
        n += 1;
    }
    table
}

const CHOOSE: [[u32; 7]; 33] = binomials();

pub fn slots(count: u32) -> u32 {
    CHOOSE[32][count as usize] << (2 * count)
}

pub(crate) fn canonical(position: Position) -> Position {
    if position.side() == Side::Black {
        position
    } else {
        position.flipped()
    }
}

pub fn rank(position: Position) -> (u32, u32) {
    let position = canonical(position);
    let mut combinadic = 0;
    let mut kind = 0;
    let mut count = 0;
    for sq in squares(position.occupied()) {
        let color = (position.mask(Side::White) & bit(sq) != 0) as u32;
        let king = (position.kings() & bit(sq) != 0) as u32;
        kind |= (color | (king << 1)) << (2 * count);
        count += 1;
        combinadic += CHOOSE[sq as usize][count as usize];
    }
    (count, (combinadic << (2 * count)) | kind)
}

pub fn unrank(count: u32, index: u32) -> Option<Position> {
    if !(1..=6).contains(&count) || index >= slots(count) {
        return None;
    }
    let mut occupancy = 0;
    let mut combination = index >> (2 * count);
    let mut sq = 31;
    for k in (1..=count as usize).rev() {
        while CHOOSE[sq][k] > combination {
            sq -= 1;
        }
        combination -= CHOOSE[sq][k];
        occupancy |= bit(sq as u8);
    }
    decode(occupancy, index)
}

fn decode(occupancy: u32, mut kind: u32) -> Option<Position> {
    let mut pieces = [0; 2];
    let mut kings = 0;
    for sq in squares(occupancy) {
        let color = kind & 1;
        let king = kind & 2 != 0;
        pieces[color as usize] |= bit(sq);
        if king {
            kings |= bit(sq);
        } else if Side::ALL[color as usize].crown(sq) {
            return None;
        }
        kind >>= 2;
    }
    Some(Position::from_valid_masks(pieces, kings, Side::Black))
}

pub(crate) fn entries(count: u32) -> impl Iterator<Item = (u32, Option<Position>)> {
    let stride = 1 << (2 * count);
    let mut occupancy = (1 << count) - 1u32;
    (0..slots(count)).map(move |slot| {
        let kind = slot % stride;
        let result = decode(occupancy, kind);
        if kind == stride - 1 && slot + 1 < slots(count) {
            let low = occupancy & occupancy.wrapping_neg();
            let next = occupancy.wrapping_add(low);
            occupancy = next | (((next ^ occupancy) >> 2) / low);
        }
        (slot, result)
    })
}
