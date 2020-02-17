use crate::{
    board::{Position, Side, bit, coordinates, squares},
    moves,
};

pub const FEATURE_COUNT: usize = 10;
pub const CLASSICAL: [i32; FEATURE_COUNT] = [100, 170, 3, -7, 4, 2, 5, 4, 8, 12];
pub const FEATURES: [&str; FEATURE_COUNT] = [
    "men",
    "kings",
    "quiet_mobility",
    "blocked",
    "promotion_tempo",
    "centrality",
    "wing_balance",
    "formation",
    "king_activity",
    "capture_danger",
];

fn side_features(position: Position, side: Side) -> [i32; FEATURE_COUNT] {
    let activity = moves::activity(position, side);
    let ours = position.mask(side);
    let kings = position.kings();
    let mut result = [0; FEATURE_COUNT];
    result[0] = (ours & !kings).count_ones() as i32;
    result[1] = (ours & kings).count_ones() as i32;
    result[2] = activity.quiet;
    result[3] = activity.blocked;
    result[9] = activity.threatened.count_ones() as i32;
    let mut wings = [0i32; 2];
    for sq in squares(ours) {
        let (row, col) = coordinates(sq);
        let centrality = i32::from(7 - ((row * 2 - 7).abs() + (col * 2 - 7).abs()) / 2);
        let king = kings & bit(sq) != 0;
        result[5] += centrality;
        if king {
            result[8] += centrality;
        } else {
            let progress = if side == Side::Black { row } else { 7 - row };
            result[4] += i32::from(progress);
        }
        wings[usize::from(col >= 4)] += 1;
        for dc in [-1, 1] {
            let neighbor = crate::board::square_at(row + 1, col + dc);
            if neighbor < 32 && ours & bit(neighbor) != 0 {
                result[7] += 1;
            }
        }
    }
    result[6] = wings[0].min(wings[1]);
    result
}

pub fn features(position: Position) -> [i32; FEATURE_COUNT] {
    let own = side_features(position, position.side());
    let other = side_features(position, position.side().other());
    std::array::from_fn(|i| own[i] - other[i])
}

pub fn linear(position: Position, weights: &[i32; FEATURE_COUNT]) -> i32 {
    features(position)
        .iter()
        .zip(weights)
        .map(|(a, b)| a * b)
        .sum::<i32>()
        .clamp(-20_000, 20_000)
}

pub trait Evaluator {
    fn evaluate(&self, position: Position) -> i32;
}

pub struct Classical;
impl Evaluator for Classical {
    fn evaluate(&self, position: Position) -> i32 {
        linear(position, &CLASSICAL)
    }
}
