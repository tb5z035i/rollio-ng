//! `probe --json` is a no-op for static-config cora drivers.

pub fn run(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let ids: Vec<&str> = Vec::new();
    if json {
        println!("{}", serde_json::to_string_pretty(&ids)?);
    } else {
        println!("gripper-cora: static-config driver; no auto-discovery (declare devices in config.toml)");
    }
    Ok(())
}
