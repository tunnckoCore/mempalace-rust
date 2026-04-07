mod artifacts;
mod cli;
mod compression;
mod config;
mod convo;
mod dialect;
mod embedding;
mod extractor;
mod graph;
mod kg;
mod layers;
mod mcp;
mod project;
mod search;
mod storage;
mod wakeup;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
