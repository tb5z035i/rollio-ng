fn main() {
    if let Err(error) = rollio::run_cli() {
        eprintln!("rollio: {error}");
        std::process::exit(1);
    }
}
