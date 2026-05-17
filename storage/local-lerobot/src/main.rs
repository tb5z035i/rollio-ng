fn main() {
    if let Err(error) = rollio_storage_local_lerobot::run_cli() {
        eprintln!("rollio-storage-local-lerobot: {error}");
        std::process::exit(1);
    }
}
