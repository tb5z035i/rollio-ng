fn main() {
    if let Err(error) = rollio_episode_assembler::run_cli() {
        eprintln!("rollio-episode-assembler: {error}");
        std::process::exit(1);
    }
}
