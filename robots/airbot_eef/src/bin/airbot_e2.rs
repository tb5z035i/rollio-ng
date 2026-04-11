use rollio_robot_airbot_eef::{DriverProfile, run_with_profile};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(error) = run_with_profile(DriverProfile::E2).await {
        eprintln!("rollio-robot-airbot-e2: {error}");
        std::process::exit(1);
    }
}
