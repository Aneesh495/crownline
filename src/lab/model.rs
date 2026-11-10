use crate::{
    board::{Position, Side, bit, coordinates, squares},
    eval::{self, Evaluator, FEATURE_COUNT},
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, fs, io::Read, path::Path};

pub const HIDDEN: usize = 24;
pub const BOARD_FEATURES: usize = 128;
pub const INPUT: usize = 160;
pub const FIRST_SCALE: i32 = 256;
pub const HEAD_SCALE: i32 = 512;
pub const SCHEMA: &str = "crownline-quantized-model-1";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Linear,
    Network,
    Blend,
    Disabled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub schema: String,
    pub seed: u64,
    pub data_sha256: String,
    pub linear: [i32; FEATURE_COUNT],
    pub input: Vec<i16>,
    pub bias: [i16; HIDDEN],
    pub head: [i16; HIDDEN],
}

impl Model {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA
            || self.input.len() != INPUT * HIDDEN
            || self.data_sha256.len() != 64
            || !self.data_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.linear.iter().any(|v| !(-1000..=1000).contains(v))
            || self
                .input
                .iter()
                .chain(&self.bias)
                .chain(&self.head)
                .any(|v| !(-4096..=4096).contains(v))
        {
            return Err("invalid model identity, dimensions or weight bounds".into());
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let file = fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(128_001)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 128_000 {
            return Err("model exceeds the file limit".into());
        }
        let result: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        result.validate()?;
        Ok(result)
    }
}

pub fn classes(position: Position) -> [u32; 4] {
    let black = position.mask(Side::Black);
    let white = position.mask(Side::White);
    let kings = position.kings();
    [black & !kings, white & !kings, black & kings, white & kings]
}

fn square_feature(class: usize, sq: u8, view: Side) -> usize {
    let (relative, square) = if view == Side::Black {
        (class, sq)
    } else {
        (class ^ 1, 31 - sq)
    };
    relative * 32 + usize::from(square)
}

pub fn context(position: Position, view: Side) -> [usize; 4] {
    let ours = position.mask(view);
    let total = position.occupied().count_ones();
    let ours_kings = (position.kings() & ours).count_ones();
    let mut adjacent = 0;
    for sq in squares(ours) {
        let (row, col) = coordinates(sq);
        for dc in [-1, 1] {
            let next = crate::board::square_at(row + 1, col + dc);
            if next < 32 && ours & bit(next) != 0 {
                adjacent += 1;
            }
        }
    }
    [
        BOARD_FEATURES + ((total - 1) / 3) as usize,
        BOARD_FEATURES + 8 + (ours.count_ones() / 2) as usize,
        BOARD_FEATURES + 16 + (ours_kings / 2) as usize,
        BOARD_FEATURES + 24 + adjacent.min(7),
    ]
}

pub fn active(position: Position, view: Side) -> Vec<usize> {
    let mut features = Vec::with_capacity(28);
    for (kind, mask) in classes(position).into_iter().enumerate() {
        features.extend(squares(mask).map(|square| square_feature(kind, square, view)));
    }
    features.extend(context(position, view));
    features
}

#[derive(Clone)]
struct Accumulator {
    masks: [u32; 4],
    lanes: [[i32; HIDDEN]; 2],
}

pub struct NativeModel<'a> {
    model: &'a Model,
    kind: ModelKind,
    cache: RefCell<Accumulator>,
}

fn add(lanes: &mut [i32; HIDDEN], weights: &[i16], feature: usize, sign: i32) {
    for (value, &weight) in lanes.iter_mut().zip(&weights[feature * HIDDEN..][..HIDDEN]) {
        *value += sign * i32::from(weight);
    }
}

impl<'a> NativeModel<'a> {
    pub fn new(model: &'a Model, kind: ModelKind) -> Self {
        Self {
            model,
            kind,
            cache: RefCell::new(Accumulator {
                masks: [0; 4],
                lanes: [[0; HIDDEN]; 2],
            }),
        }
    }

    fn network(&self, position: Position) -> i32 {
        let masks = classes(position);
        let mut cache = self.cache.borrow_mut();
        for (kind, &mask) in masks.iter().enumerate() {
            let changed = mask ^ cache.masks[kind];
            for square in squares(changed) {
                let sign = if mask & bit(square) == 0 { -1 } else { 1 };
                for view in Side::ALL {
                    add(
                        &mut cache.lanes[view as usize],
                        &self.model.input,
                        square_feature(kind, square, view),
                        sign,
                    );
                }
            }
        }
        cache.masks = masks;
        let score = |view: Side| {
            let mut lanes = cache.lanes[view as usize];
            for feature in context(position, view) {
                add(&mut lanes, &self.model.input, feature, 1);
            }
            raw(lanes, self.model)
        };
        scale(score(position.side()) - score(position.side().other()))
    }
}

fn raw(lanes: [i32; HIDDEN], model: &Model) -> i32 {
    lanes
        .into_iter()
        .zip(model.bias)
        .zip(model.head)
        .map(|((input, bias), head)| {
            (input + i32::from(bias)).clamp(0, FIRST_SCALE) * i32::from(head)
        })
        .sum()
}

fn scale(difference: i32) -> i32 {
    ((i64::from(difference) * 100) / i64::from(FIRST_SCALE * HEAD_SCALE)).clamp(-20_000, 20_000)
        as i32
}

pub fn exported_network(position: Position, model: &Model) -> i32 {
    let forward = |view| {
        let active = active(position, view);
        let mut sum = 0;
        for lane in 0..HIDDEN {
            let input: i32 = active
                .iter()
                .map(|&feature| i32::from(model.input[feature * HIDDEN + lane]))
                .sum();
            let activation = (input + i32::from(model.bias[lane])).clamp(0, FIRST_SCALE);
            sum += activation * i32::from(model.head[lane]);
        }
        sum
    };
    scale(forward(position.side()) - forward(position.side().other()))
}

impl Evaluator for NativeModel<'_> {
    fn evaluate(&self, position: Position) -> i32 {
        match self.kind {
            ModelKind::Linear => eval::linear(position, &self.model.linear),
            ModelKind::Network => self.network(position),
            ModelKind::Blend => {
                (self.network(position) + eval::linear(position, &self.model.linear)) / 2
            }
            ModelKind::Disabled => 0,
        }
    }
}
