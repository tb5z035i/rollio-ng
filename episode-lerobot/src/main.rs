fn main() {
    if let Err(error) = rollio_episode_lerobot::run_cli() {
        eprintln!("rollio-episode-lerobot: {error}");
        std::process::exit(1);
    }
}
