use crownline::lab::{
    Split,
    arena::{self, Profile},
    model::ModelKind,
    selfplay, train, verify,
};
use serde::Serialize;
use std::path::Path;

fn show(value: impl Serialize) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn kind(text: &str) -> Result<ModelKind, String> {
    match text {
        "linear" => Ok(ModelKind::Linear),
        "network" => Ok(ModelKind::Network),
        "blend" => Ok(ModelKind::Blend),
        _ => Err("expected linear, network or blend".into()),
    }
}

fn number<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("invalid numeric argument {text}"))
}

fn run(args: &[String]) -> Result<(), String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    match args.as_slice() {
        ["verify-historical", manifest, sources, training, models, directory] => show(crownline::lab::verify_historical::verify(
            Path::new(manifest), Path::new(sources), Path::new(training), Path::new(models), Path::new(directory))?),
        ["historical", manifest, sources, training, models, directory] => {
            let report = crownline::lab::historical::run(Path::new(manifest), Path::new(sources), Path::new(training), Path::new(models), Path::new(directory))?;
            let rejected = report.rejected_games;
            show(report)?;
            if rejected > 0 { return Err(format!("{rejected} historical records failed replay")); }
            Ok(())
        }
        ["tactics"] => show(crownline::lab::tactics::run()?),
        ["openings"] => show(crownline::lab::openings()),
        ["selfplay", directory, games] => show(selfplay::generate(Path::new(directory), number(games)?)?),
        ["train", data, directory, seed, epochs, fraction] => show(train::run(
            Path::new(data), Path::new(directory), number(seed)?, number(epochs)?, number(fraction)?,
        )?),
        ["arena", model, directory, variant, profile, split, pairs] => {
            let profile = match *profile {
                "nodes192" => Profile::Nodes192, "nodes768" => Profile::Nodes768, "time1ms" => Profile::Time1ms,
                _ => return Err("unknown profile".into()),
            };
            let split = match *split {
                "test" => Split::Test, "validation" => Split::Validation,
                _ => return Err("expected test or validation".into()),
            };
            show(arena::run(Path::new(model), Path::new(directory), kind(variant)?, profile, split, number(pairs)?)?)
        }
        ["reference", engine, directory, pairs] => show(crownline::lab::reference::run(
            Path::new(engine), Path::new(directory), number(pairs)?,
        )?),
        ["verify-reference", engine, directory] => show(verify::verify_reference(Path::new(engine), Path::new(directory))?),
        ["verify-arena", model, directory] => show(verify::verify_arena(Path::new(model), Path::new(directory))?),
        ["verify-playthroughs", data] => show(verify::verify_playthroughs(Path::new(data))?),
        ["verify-data", data] => show(verify::verify_dataset(Path::new(data))?),
        ["verify-model", data, model] => show(verify::verify_model(Path::new(data), Path::new(model))?),
        _ => Err("commands: historical MANIFEST SOURCES TRAINING MODELS OUTPUT | verify-historical MANIFEST SOURCES TRAINING MODELS OUTPUT | openings | selfplay DIR GAMES | train DATA DIR SEED EPOCHS FRACTION | arena MODEL DIR KIND PROFILE SPLIT PAIRS | verify-arena MODEL DIR | verify-data DATA | verify-model DATA DIR".into()),
    }
}

fn main() {
    if let Err(error) = run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("crownline-lab: {error}");
        std::process::exit(1);
    }
}
