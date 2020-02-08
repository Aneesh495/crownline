use crownline::{board::Position, moves, tablebase};
use std::path::Path;

fn main() {
    if let Err(error) = run() {
        eprintln!("crownline: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    match command.as_str() {
        "perft" => {
            let depth = args
                .next()
                .ok_or("perft DEPTH [FEN]")?
                .parse::<u8>()
                .map_err(|e| e.to_string())?;
            if depth > 10 {
                return Err("CLI perft depth limit is 10".into());
            }
            let position = args
                .next()
                .map(|fen| fen.parse())
                .transpose()?
                .unwrap_or_else(Position::start);
            println!("{}", moves::perft(position, depth));
        }
        "legal" => {
            let position = args
                .next()
                .map(|fen| fen.parse())
                .transpose()?
                .unwrap_or_else(Position::start);
            for turn in moves::legal(position) {
                println!("{turn}");
            }
        }
        "solve" => {
            let path = args.next().ok_or("solve DIRECTORY COUNT")?;
            let count = args
                .next()
                .ok_or("solve DIRECTORY COUNT")?
                .parse::<u32>()
                .map_err(|e| e.to_string())?;
            let result = tablebase::build(Path::new(&path), count)?;
            println!(
                "{}",
                serde_json::to_string_pretty(result.inventory()).map_err(|e| e.to_string())?
            );
        }
        "verify-table" => {
            let path = args.next().ok_or("verify-table DIRECTORY")?;
            let table = tablebase::Tablebase::load(Path::new(&path))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&tablebase::verify(&table)?)
                    .map_err(|e| e.to_string())?
            );
        }
        "probe" => {
            let path = args.next().ok_or("probe DIRECTORY FEN")?;
            let position = args.next().ok_or("probe DIRECTORY FEN")?.parse()?;
            let table = tablebase::Tablebase::load(Path::new(&path))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&table.probe(position)).map_err(|e| e.to_string())?
            );
        }
        _ => return Err("commands: perft, legal, solve, verify-table, probe".into()),
    }
    if args.next().is_some() {
        return Err("unexpected extra argument".into());
    }
    Ok(())
}
