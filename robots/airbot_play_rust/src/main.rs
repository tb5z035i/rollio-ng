use clap::Parser;
use rollio_robot_airbot_play::{run_cli, Cli};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(error) = run_cli(cli).await {
        eprintln!("rollio-robot-airbot-play: {error}");
        std::process::exit(1);
    }
}
