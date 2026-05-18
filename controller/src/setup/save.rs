use rollio_types::config::ProjectConfig;
use std::error::Error;
use std::fs;
use std::path::Path;

pub(super) fn save_project_config(
    project: &ProjectConfig,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(output_path, toml::to_string_pretty(project)?)?;
    Ok(())
}
