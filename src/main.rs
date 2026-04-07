fn main() {
    if let Err(err) = mempalace_rust::cli::run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
