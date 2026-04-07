use crate::bench::{backend_help, load_dataset, run_benchmark, BenchmarkBackend};
use crate::compression::maintain_compressed_artifacts;
use crate::config::AppConfig;
use crate::convo;
use crate::mcp;
use crate::project;
use crate::storage::Storage;
use crate::wakeup;
use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mempalace-rust",
    version,
    about = "Rust migration of MemPalace"
)]
struct Cli {
    #[arg(long)]
    palace: Option<PathBuf>,
    #[arg(long)]
    embedding_backend: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init(InitArgs),
    Mine(MineArgs),
    Search(SearchArgs),
    Status,
    WakeUp(WakeUpArgs),
    Compress(CompressArgs),
    Mcp(McpArgs),
    Benchmark(BenchmarkArgs),
}

#[derive(Args)]
struct InitArgs {
    dir: PathBuf,
}

#[derive(Copy, Clone, Eq, PartialEq, ValueEnum)]
enum MineMode {
    Projects,
    Convos,
}

#[derive(Args)]
struct MineArgs {
    dir: PathBuf,
    #[arg(long, value_enum, default_value = "projects")]
    mode: MineMode,
    #[arg(long)]
    wing: Option<String>,
    #[arg(long, default_value = "mempalace")]
    agent: String,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, default_value = "exchange")]
    extract: String,
}

#[derive(Args)]
struct SearchArgs {
    query: String,
    #[arg(long)]
    wing: Option<String>,
    #[arg(long)]
    room: Option<String>,
    #[arg(long, default_value_t = 5)]
    results: usize,
}

#[derive(Args)]
struct WakeUpArgs {
    #[arg(long)]
    wing: Option<String>,
}

#[derive(Args)]
struct CompressArgs {
    #[arg(long)]
    wing: Option<String>,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args)]
struct McpArgs {
    #[arg(long, default_value = "stdio")]
    transport: String,
}

#[derive(Args)]
struct BenchmarkArgs {
    dataset: PathBuf,
    #[arg(long, value_enum, default_value = "hybrid")]
    backend: BenchmarkBackend,
    #[arg(long, default_value_t = 5)]
    k: usize,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut config = AppConfig::load(cli.palace.as_deref())?;
    if let Some(backend) = cli.embedding_backend {
        config.embedding_backend = backend;
        std::env::set_var("MEMPALACE_EMBEDDING_BACKEND", &config.embedding_backend);
    } else {
        std::env::set_var("MEMPALACE_EMBEDDING_BACKEND", &config.embedding_backend);
    }
    match cli.command {
        Commands::Init(args) => {
            config.init_files()?;
            let cfg_existed = args.dir.join("mempalace.yaml").exists();
            let cfg_path = project::init_project(&args.dir)?;
            if cfg_existed {
                println!("existing config preserved: {}", cfg_path.display());
            } else {
                println!("initialized config: {}", cfg_path.display());
            }
            println!("global config: {}", config.config_file.display());
            println!("palace path: {}", config.palace_path.display());
            println!("embedding backend: {}", config.embedding_backend);
        }
        Commands::Mine(args) => {
            let mut storage = Storage::open(&config.palace_path)?;
            match args.mode {
                MineMode::Projects => {
                    let summary = project::mine_project(
                        &args.dir,
                        &mut storage,
                        args.wing.as_deref(),
                        &args.agent,
                        args.limit,
                        args.dry_run,
                    )?;
                    print_mine_summary(
                        "projects",
                        &summary.wing,
                        summary.files_seen,
                        summary.files_skipped,
                        summary.drawers_added,
                        &summary.room_counts,
                    );
                }
                MineMode::Convos => {
                    let summary = convo::mine_conversations(
                        &args.dir,
                        &mut storage,
                        args.wing.as_deref(),
                        &args.agent,
                        args.limit,
                        args.dry_run,
                        &args.extract,
                    )?;
                    print_mine_summary(
                        "conversations",
                        &summary.wing,
                        summary.files_seen,
                        summary.files_skipped,
                        summary.drawers_added,
                        &summary.room_counts,
                    );
                }
            }
        }
        Commands::Search(args) => {
            let storage = Storage::open(&config.palace_path)?;
            if args.query.trim().is_empty() {
                anyhow::bail!("search query must not be empty");
            }
            let hits = storage.search(
                &args.query,
                args.wing.as_deref(),
                args.room.as_deref(),
                args.results,
            )?;
            if hits.is_empty() {
                println!("No results found for: {}", args.query);
            } else {
                println!("Results for: {}", args.query);
                for (idx, hit) in hits.iter().enumerate() {
                    println!("[{}] {} / {}", idx + 1, hit.wing, hit.room);
                    println!("    Source: {}", hit.source_file);
                    println!(
                        "    Score:  fused={:.3} lex={:.3} sem={:.3} heur={:.3} backend={}",
                        hit.fused_score,
                        hit.lexical_score,
                        hit.semantic_score,
                        hit.heuristic_score,
                        hit.embedding_backend
                    );
                    println!("    {}", hit.snippet.replace('\n', "\n    "));
                }
            }
        }
        Commands::Status => {
            let storage = Storage::open(&config.palace_path)?;
            let report = storage.status()?;
            println!("MemPalace status — {} drawers", report.total_drawers);
            for (wing, rooms) in report.by_wing {
                println!("WING: {}", wing);
                for (room, count) in rooms {
                    println!("  ROOM: {:20} {}", room, count);
                }
            }
        }
        Commands::WakeUp(args) => {
            let storage = Storage::open(&config.palace_path)?;
            let text = wakeup::render_wakeup(&config, &storage, args.wing.as_deref())?;
            println!("{}", text);
        }
        Commands::Compress(args) => {
            let mut storage = Storage::open(&config.palace_path)?;
            let stats =
                maintain_compressed_artifacts(&mut storage, args.wing.as_deref(), args.dry_run)?;
            println!(
                "Compression total: {} -> {} chars (artifacts written: {})",
                stats.total_original_chars, stats.total_compressed_chars, stats.artifacts_written
            );
        }
        Commands::Mcp(args) => {
            if args.transport != "stdio" {
                anyhow::bail!("only --transport stdio is supported currently");
            }
            mcp::run_stdio_server(&config)?;
        }
        Commands::Benchmark(args) => {
            let mut storage = Storage::open(&config.palace_path)?;
            let dataset = load_dataset(&args.dataset)?;
            let result = run_benchmark(&mut storage, &dataset, args.backend, args.k)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            println!("{}", backend_help());
        }
    }
    Ok(())
}

fn print_mine_summary(
    label: &str,
    wing: &str,
    files_seen: usize,
    files_skipped: usize,
    drawers_added: usize,
    room_counts: &std::collections::HashMap<String, usize>,
) {
    println!("MemPalace mine — {}", label);
    println!("Wing: {}", wing);
    println!("Files seen: {}", files_seen);
    println!("Files skipped: {}", files_skipped);
    println!("Drawers added: {}", drawers_added);
    if !room_counts.is_empty() {
        println!("By room:");
        let mut rooms: Vec<_> = room_counts.iter().collect();
        rooms.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (room, count) in rooms {
            println!("  {:20} {}", room, count);
        }
    }
}
