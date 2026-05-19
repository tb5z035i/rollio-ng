fn main() {
    if let Err(error) = rollio_storage_dataloop::run_cli() {
        eprintln!("rollio-storage-dataloop: {error}");
        std::process::exit(1);
    }
}
