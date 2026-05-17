//! `probe --json` is a no-op for static-config cora drivers — controller never
//! auto-spawns them.

pub fn run(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let ids: Vec<&str> = Vec::new();
    if json {
        println!("{}", serde_json::to_string_pretty(&ids)?);
    } else {
        println!("tactile-cora: static-config driver; no auto-discovery (declare devices in config.toml)");
    }
    Ok(())
}
