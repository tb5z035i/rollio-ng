fn main() {
    if let Err(error) = rollio_storage_local::run_cli() {
        eprintln!("rollio-storage-local: {error}");
        std::process::exit(1);
    }
}
