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
        "explore" | "verify-frontier" => {
            let directory = args.next().ok_or("explore|verify-frontier TABLE PARTITION [LIMIT FEN]")?;
            let path = args.next().ok_or("missing partition path")?;
            let table = tablebase::Tablebase::load(Path::new(&directory))?;
            let stats = if command == "explore" {
                let limit = args.next().ok_or("missing expansion limit")?.parse::<usize>().map_err(|e| e.to_string())?;
                let start = args.next().ok_or("missing starting FEN")?.parse()?;
                tablebase::frontier::build(Path::new(&path), start, limit, &table)?
            } else {
                tablebase::frontier::verify(Path::new(&path), &table)?
            };
            println!("{}", serde_json::to_string_pretty(&stats).map_err(|e| e.to_string())?);
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
        "wire" => return crownline::wire::serve(args.next().map(Into::into)).map_err(|e| e.to_string()),
        "search" | "search-proof" => {
            use crownline::{eval::Classical, game::History, search::{Config, Limits, Rules, analyze_with_table}};
            let table = if command == "search-proof" {
                let directory = args.next().ok_or("search-proof TABLE DEPTH NODES [FEN]")?;
                Some(tablebase::Tablebase::load(Path::new(&directory))?)
            } else {
                None
            };
            let depth = args.next().ok_or("search DEPTH NODES [FEN]")?.parse::<u8>().map_err(|e| e.to_string())?;
            let nodes = args.next().ok_or("search DEPTH NODES [FEN]")?.parse::<u64>().map_err(|e| e.to_string())?;
            if nodes > 50_000_000 {
                return Err("CLI node limit is 50 million".into());
            }
            let position = args.next().map(|fen| fen.parse()).transpose()?.unwrap_or_else(Position::start);
            let rules = if table.is_some() { Rules::Board } else { Rules::House };
            let config = Config { rules, ..Config::default() };
            let result = analyze_with_table(
                &History::new(position), &Classical, Limits::fixed(depth, nodes), config, table.as_ref(),
            );
            println!("{}", serde_json::to_string_pretty(&crownline::wire::result_view(&result)).map_err(|e| e.to_string())?);
        }
        "review" | "pdn" => {
            use crownline::{pdn, review};
            let filename = args.next().ok_or("review|pdn INPUT [TABLE|--partial]")?;
            let path = Path::new(&filename);
            if path.metadata().map_err(|e| e.to_string())?.len() > pdn::MAX_INPUT as u64 { return Err("PDN file exceeds cap".into()); }
            let input = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            let option = args.next();
            let record = pdn::import(&input, command == "pdn" && option.as_deref() == Some("--partial")).map_err(|e| e.to_string())?;
            if let Some(diagnostic) = &record.diagnostic { eprintln!("{diagnostic}"); }
            if command == "review" {
                let tables = option.map(|p| tablebase::Tablebase::load(Path::new(&p))).transpose()?;
                println!("{}", serde_json::to_string_pretty(&review::review(&record, tables.as_ref(), 1000)?).map_err(|e| e.to_string())?);
            } else {
                print!("{}", pdn::export(&record));
            }
        }
        "witness" => {
            let directory = args.next().ok_or("witness TABLE FEN")?;
            let position = args.next().ok_or("witness TABLE FEN")?.parse()?;
            let table = tablebase::Tablebase::load(Path::new(&directory))?;
            let witness = crownline::review::witness(position, &table, 32)?;
            println!("{}", serde_json::to_string_pretty(&witness).map_err(|e| e.to_string())?);
        }
        _ => return Err("commands: perft, legal, solve, verify-table, explore, verify-frontier, probe, search, search-proof, pdn, review, witness, wire".into()),
    }
    if args.next().is_some() {
        return Err("unexpected extra argument".into());
    }
    Ok(())
}
