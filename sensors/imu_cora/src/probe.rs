//! `probe --json` emits a deterministic JSON list of device IDs this driver can
//! produce. cora-* drivers are static-config (the user wires them up in
//! `config.toml`), so probe returns an empty list — controller never auto-spawns
//! cora devices, it only `query`s the ones the user declared.

use serde_json::json;

pub fn run(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let ids: Vec<&str> = Vec::new();
    if json {
        println!("{}", serde_json::to_string_pretty(&ids)?);
    } else {
        let _ = json!(ids);
        println!("imu-cora: static-config driver; no auto-discovery (declare devices in config.toml)");
    }
    Ok(())
}
