fn main() {
    if let Err(error) = rollio_storage::run_cli() {
        eprintln!("rollio-storage: {error}");
        std::process::exit(1);
    }
}
