fn main() {
    if let Err(error) = rollio_teleop_router::run_cli() {
        eprintln!("rollio-teleop-router: {error}");
        std::process::exit(1);
    }
}
