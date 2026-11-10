use super::{
    Rng, digest, file_hash,
    model::{self, FIRST_SCALE, HEAD_SCALE, HIDDEN, INPUT, Model, ModelKind, NativeModel},
    selfplay::{Dataset, Sample, samples},
};
use crate::{
    board::Position,
    eval::{Classical, Evaluator, FEATURE_COUNT, features},
    tablebase::atomic,
};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

const NORMALIZE: [f64; FEATURE_COUNT] = [3., 3., 16., 5., 20., 32., 4., 10., 16., 6.];
const SCHEMA: &str = "crownline-training-v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema: String,
    seed: u64,
    fraction: usize,
    rng: Rng,
    data_sha256: String,
    completed: usize,
    weights: [f64; FEATURE_COUNT],
    inputs: Vec<f64>,
    bias: [f64; HIDDEN],
    head: [f64; HIDDEN],
    initial_weights_sha256: String,
    losses: Vec<f64>,
}

fn jitter(rng: &mut Rng, size: f64) -> f64 {
    (rng.uniform() - 0.5) * 2.0 * size
}

impl State {
    fn new(seed: u64, fraction: usize, data_sha256: String) -> Result<Self, String> {
        let mut rng = Rng(seed);
        let mut state = Self {
            schema: SCHEMA.into(),
            seed,
            fraction,
            rng,
            data_sha256,
            completed: 0,
            weights: std::array::from_fn(|_| jitter(&mut rng, 0.03)),
            inputs: (0..INPUT * HIDDEN)
                .map(|_| jitter(&mut rng, 0.05))
                .collect(),
            bias: std::array::from_fn(|_| 0.32 + jitter(&mut rng, 0.04)),
            head: std::array::from_fn(|_| jitter(&mut rng, 0.1)),
            initial_weights_sha256: String::new(),
            losses: Vec::new(),
        };
        state.rng = rng;
        state.initial_weights_sha256 = state.weight_hash()?;
        Ok(state)
    }

    fn weight_hash(&self) -> Result<String, String> {
        let bytes = serde_json::to_vec(&(&self.weights, &self.inputs, &self.bias, &self.head))
            .map_err(|e| e.to_string())?;
        Ok(digest(&bytes))
    }

    fn check(&self, seed: u64, fraction: usize, hash: &str, epochs: usize) -> Result<(), String> {
        if self.schema != SCHEMA
            || self.seed != seed
            || self.fraction != fraction
            || self.data_sha256 != hash
            || self.completed > epochs
            || self.losses.len() != self.completed
            || self.rng.0 == 0
            || self.inputs.len() != HIDDEN * INPUT
            || self
                .inputs
                .iter()
                .chain(&self.weights)
                .chain(&self.bias)
                .chain(&self.head)
                .chain(&self.losses)
                .any(|v| !v.is_finite())
        {
            return Err("checkpoint identity or numerical contract mismatch".into());
        }
        Ok(())
    }

    fn activate(&self, features: &[usize]) -> [f64; HIDDEN] {
        let mut lanes = self.bias;
        for &feature in features {
            for (lane, &weight) in lanes
                .iter_mut()
                .zip(&self.inputs[feature * HIDDEN..][..HIDDEN])
            {
                *lane += weight;
            }
        }
        lanes
    }

    fn train(&mut self, sample: &Sample, lr: f64) -> f64 {
        let position = sample.position;
        let label = f64::from(sample.target) / 100.0;
        let normalized = features(position)
            .into_iter()
            .zip(NORMALIZE)
            .map(|(f, n)| f64::from(f) / n)
            .collect::<Vec<_>>();
        let linear_error = normalized
            .iter()
            .zip(self.weights)
            .map(|(x, w)| x * w)
            .sum::<f64>()
            - label;
        for (weight, feature) in self.weights.iter_mut().zip(normalized) {
            *weight = (*weight - lr * (linear_error.clamp(-4., 4.) * feature + 0.001 * *weight))
                .clamp(-8., 8.);
        }

        let a = model::active(position, position.side());
        let b = model::active(position, position.side().other());
        let first = self.activate(&a);
        let second = self.activate(&b);
        let activations: [f64; HIDDEN] =
            std::array::from_fn(|i| first[i].clamp(0., 1.) - second[i].clamp(0., 1.));
        let error = self
            .head
            .iter()
            .zip(activations)
            .map(|(w, a)| w * a)
            .sum::<f64>()
            - label;
        let grad = lr * error.clamp(-4., 4.);
        for i in 0..HIDDEN {
            let head = self.head[i];
            let first_grad = if (0.0..1.0).contains(&first[i]) {
                grad * head
            } else {
                0.
            };
            let second_grad = if (0.0..1.0).contains(&second[i]) {
                -grad * head
            } else {
                0.
            };
            self.bias[i] = (self.bias[i] - first_grad - second_grad).clamp(-2., 2.);
            for &feature in &a {
                let w = &mut self.inputs[feature * HIDDEN + i];
                *w = (*w - first_grad).clamp(-4., 4.);
            }
            for &feature in &b {
                let w = &mut self.inputs[feature * HIDDEN + i];
                *w = (*w - second_grad).clamp(-4., 4.);
            }
            self.head[i] = (head - grad * activations[i] - lr * 0.0005 * head).clamp(-5., 5.);
        }
        error * error
    }

    fn quantize(&self) -> Model {
        let encode = |value: f64, scale: i32| (value * f64::from(scale)).round() as i16;
        Model {
            schema: model::SCHEMA.into(),
            seed: self.seed,
            data_sha256: self.data_sha256.clone(),
            linear: std::array::from_fn(|i| (self.weights[i] * 100. / NORMALIZE[i]).round() as i32),
            input: self
                .inputs
                .iter()
                .map(|&weight| encode(weight, FIRST_SCALE))
                .collect(),
            bias: self.bias.map(|weight| encode(weight, FIRST_SCALE)),
            head: self.head.map(|weight| encode(weight, HEAD_SCALE)),
        }
    }
}

#[derive(Serialize)]
struct Metrics {
    n: usize,
    classical_rmse: f64,
    linear_rmse: f64,
    network_rmse: f64,
    blend_rmse: f64,
    disabled_rmse: f64,
}

fn rmse(samples: &[Sample], evaluator: &dyn Evaluator) -> f64 {
    (samples
        .iter()
        .map(|sample| f64::from(sample.target - evaluator.evaluate(sample.position)).powi(2))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt()
}

fn metrics(samples: &[Sample], model: &Model) -> Metrics {
    Metrics {
        n: samples.len(),
        classical_rmse: rmse(samples, &Classical),
        linear_rmse: rmse(samples, &NativeModel::new(model, ModelKind::Linear)),
        network_rmse: rmse(samples, &NativeModel::new(model, ModelKind::Network)),
        blend_rmse: rmse(samples, &NativeModel::new(model, ModelKind::Blend)),
        disabled_rmse: rmse(samples, &NativeModel::new(model, ModelKind::Disabled)),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub fen: String,
    pub network: i32,
    pub linear: i32,
    pub blended: i32,
}

pub fn check_probe(probe: &Probe, model: &Model) -> Result<(), String> {
    let position: Position = probe.fen.parse()?;
    let exported = model::exported_network(position, model);
    let native = NativeModel::new(model, ModelKind::Network);
    if probe.network != exported
        || probe.network != native.evaluate(position)
        || probe.network != native.evaluate(position.flipped())
        || probe.linear != NativeModel::new(model, ModelKind::Linear).evaluate(position)
        || probe.blended != NativeModel::new(model, ModelKind::Blend).evaluate(position)
        || NativeModel::new(model, ModelKind::Disabled).evaluate(position) != 0
    {
        return Err(format!("model probe mismatch at {position}"));
    }
    let switched = Position::new(
        position.mask(crate::board::Side::Black),
        position.mask(crate::board::Side::White),
        position.kings(),
        position.side().other(),
    )?;
    if probe.network != -native.evaluate(switched) {
        return Err("network perspective sign mismatch".into());
    }
    Ok(())
}

pub fn run(
    ledger: &Path,
    directory: &Path,
    seed: u64,
    epochs: usize,
    fraction: usize,
) -> Result<serde_json::Value, String> {
    if seed == 0 || !(1..=40).contains(&epochs) || !matches!(fraction, 25 | 50 | 100) {
        return Err(
            "training requires a nonzero seed, 1 through 40 epochs, and fraction 25, 50 or 100"
                .into(),
        );
    }
    let start = std::time::Instant::now();
    let Dataset {
        train,
        validation,
        test,
        cross_split,
    } = samples(ledger)?;
    if train.is_empty() || validation.is_empty() || test.is_empty() {
        return Err("all three grouped splits require independent samples".into());
    }
    let data_sha256 = file_hash(ledger)?;
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let checkpoint = directory.join("checkpoint.json");
    let mut state = if checkpoint.exists() {
        if checkpoint.metadata().map_err(|e| e.to_string())?.len() > 512_000 {
            return Err("checkpoint too large".into());
        }
        serde_json::from_reader(fs::File::open(&checkpoint).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
    } else {
        State::new(seed, fraction, data_sha256.clone())?
    };
    state.check(seed, fraction, &data_sha256, epochs)?;
    let mut indices = (0..train.len())
        .filter(|&i| i % 100 < fraction)
        .collect::<Vec<_>>();
    let baseline_indices = indices.clone();
    for epoch in state.completed..epochs {
        indices.copy_from_slice(&baseline_indices);
        for i in (1..indices.len()).rev() {
            indices.swap(i, state.rng.sample() as usize % (i + 1));
        }
        let lr = 0.024 / (1. + epoch as f64 * 0.16);
        let loss = indices
            .iter()
            .map(|&i| state.train(&train[i], lr))
            .sum::<f64>()
            / indices.len() as f64;
        if !loss.is_finite() {
            return Err("nonfinite training loss".into());
        }
        state.completed += 1;
        state.losses.push(loss);
        atomic(&checkpoint, |file| {
            serde_json::to_writer(file, &state).map_err(io::Error::other)
        })
        .map_err(|e| e.to_string())?;
    }
    let trained_weights_sha256 = state.weight_hash()?;
    if state.initial_weights_sha256 == trained_weights_sha256 {
        return Err("training did not update parameters".into());
    }
    let model = state.quantize();
    model.validate()?;
    let model_path = directory.join("model.json");
    atomic(&model_path, |file| {
        serde_json::to_writer(file, &model).map_err(io::Error::other)
    })
    .map_err(|e| e.to_string())?;
    let native = NativeModel::new(&model, ModelKind::Network);
    let mut probes = Vec::new();
    for example in validation
        .iter()
        .step_by((validation.len() / 24).max(1))
        .take(24)
    {
        let position = example.position;
        let probe = Probe {
            fen: position.to_string(),
            network: native.evaluate(position),
            linear: NativeModel::new(&model, ModelKind::Linear).evaluate(position),
            blended: NativeModel::new(&model, ModelKind::Blend).evaluate(position),
        };
        check_probe(&probe, &model)?;
        probes.push(probe);
    }
    if !probes.iter().any(|probe| probe.network != 0) {
        return Err("disabled model matches every probe".into());
    }
    atomic(&directory.join("probes.json"), |file| {
        serde_json::to_writer(file, &probes).map_err(io::Error::other)
    })
    .map_err(|e| e.to_string())?;
    let report = serde_json::json!({
        "schema": SCHEMA, "seed": seed, "epochs": epochs, "fraction": fraction, "train_samples": indices.len(),
        "data_sha256": data_sha256, "model_sha256": file_hash(&model_path)?, "probe_count": probes.len(),
        "initial_weights_sha256": state.initial_weights_sha256, "trained_weights_sha256": trained_weights_sha256,
        "losses": state.losses, "removed_cross_split": cross_split,
        "validation": metrics(&validation, &model), "test": metrics(&test, &model),
        "elapsed_this_call_ms": start.elapsed().as_millis(),
    });
    atomic(&directory.join("run.json"), |file| {
        serde_json::to_writer_pretty(file, &report).map_err(io::Error::other)
    })
    .map_err(|e| e.to_string())?;
    Ok(report)
}
