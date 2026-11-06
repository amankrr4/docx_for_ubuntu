use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use docxlite::{LoadError, load_document};

#[derive(Debug, Default)]
struct Cli {
    files: Vec<PathBuf>,
    dump_text: Option<PathBuf>,
    verbose: bool,
    stats: bool,
}

fn main() {
    let code = run();
    std::process::exit(code);
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_args(&args) {
        Ok(Some(cli)) => cli,
        Ok(None) => return 0,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };

    if let Some(path) = cli.dump_text {
        return dump_text(&path, cli.verbose, cli.stats);
    }

    run_gui(cli.files)
}

#[cfg(feature = "gui")]
fn run_gui(files: Vec<PathBuf>) -> i32 {
    let files: Vec<String> = files
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();
    let code = docxlite::ui::run(files);
    i32::from(code.get())
}

#[cfg(not(feature = "gui"))]
fn run_gui(_files: Vec<PathBuf>) -> i32 {
    eprintln!("This build was made without the GUI feature. Use --dump-text.");
    2
}

fn dump_text(path: &Path, verbose: bool, stats: bool) -> i32 {
    let cancel = Arc::new(AtomicBool::new(false));
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load_document(path, cancel)));
    let document = match result {
        Ok(Ok(document)) => document,
        Ok(Err(error)) => {
            eprintln!("{}", error.message());
            return 1;
        }
        Err(_) => {
            eprintln!("{}", LoadError::Internal.message());
            return 1;
        }
    };
    print!("{}", document.dump_text());
    if verbose {
        for warning in &document.warnings {
            eprintln!("warning: {} ({})", warning.kind.message(), warning.count);
        }
    }
    if stats {
        eprintln!(
            "stats: nodes={} xml_bytes={} images={} table_cells={}",
            document.stats.nodes,
            document.stats.xml_bytes,
            document.stats.images,
            document.stats.table_cells
        );
    }
    0
}

fn parse_args(args: &[String]) -> Result<Option<Cli>, String> {
    let mut cli = Cli::default();
    let mut dump_mode = false;
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                return Ok(None);
            }
            "--version" | "-V" => {
                println!("docxlite {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--dump-text" => dump_mode = true,
            "--verbose" => cli.verbose = true,
            "--stats" => cli.stats = true,
            value if value.starts_with("--") => {
                return Err(format!("unknown option: {value}"));
            }
            value if dump_mode && cli.dump_text.is_none() => {
                cli.dump_text = Some(PathBuf::from(value));
            }
            value => cli.files.push(PathBuf::from(value)),
        }
    }
    if dump_mode && cli.dump_text.is_none() {
        return Err("--dump-text needs a FILE argument".to_string());
    }
    Ok(Some(cli))
}

fn print_help() {
    println!(
        "docxlite {}\n\nUsage:\n  docxlite [FILE...]\n  docxlite --dump-text FILE [--verbose] [--stats]\n\nOptions:\n  -h, --help       Show this help\n  -V, --version    Show the version\n  --dump-text FILE Print extracted text without starting the GUI\n  --verbose        Print warning categories with --dump-text\n  --stats          Print parse statistics with --dump-text",
        env!("CARGO_PKG_VERSION")
    );
}
