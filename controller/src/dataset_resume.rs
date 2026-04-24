//! Probe an existing dataset directory so `rollio collect` can resume episode
//! indexing instead of always starting at 0.
//!
//! Resume is only allowed when `meta/info.json` carries the same
//! `embedded_config_toml` as the current project (serialized the same way as
//! at assembly time). Otherwise we refuse to run if episode artifacts already
//! exist, so the operator is not left with silent overwrites starting at 0.

use rollio_types::config::{EpisodeFormat, ProjectConfig};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatasetResumeHint {
    pub next_episode_index: u32,
    pub prior_stored_episode_count: u32,
}

pub(crate) fn probe_resume(
    config: &ProjectConfig,
    invocation_cwd: &Path,
) -> Result<Option<DatasetResumeHint>, Box<dyn std::error::Error>> {
    let Some(root) = resolve_storage_root(config, invocation_cwd) else {
        return Ok(None);
    };
    if !root.is_dir() {
        return Ok(None);
    }

    match config.episode.format {
        EpisodeFormat::LeRobotV2_1 | EpisodeFormat::LeRobotV3_0 => {
            let hint = match lerobot_resume_hint_if_episodes_exist(&root)? {
                None => return Ok(None),
                Some(h) => h,
            };
            let current_toml = toml::to_string(config)?;
            let Some(stored_toml) = read_info_embedded_config_toml(&root)? else {
                return Err(format!(
                    "dataset at {} contains episodes but meta/info.json is missing; \
                     cannot verify embedded_config_toml. Use a new storage.output_path \
                     or remove the dataset before collecting.",
                    root.display()
                )
                .into());
            };
            if stored_toml.trim().is_empty() {
                return Err(format!(
                    "dataset at {} contains episodes but meta/info.json has no \
                     embedded_config_toml; cannot verify the recording config. \
                     Use a new storage.output_path or remove the dataset before collecting.",
                    root.display()
                )
                .into());
            }
            if !embedded_configs_match(&stored_toml, &current_toml) {
                return Err(format!(
                    "dataset at {} was recorded with a different project config \
                     (meta/info.json embedded_config_toml does not match the current project). \
                     Use a new storage.output_path or remove the dataset before collecting.",
                    root.display()
                )
                .into());
            }
            Ok(Some(hint))
        }
        EpisodeFormat::Mcap => {
            eprintln!(
                "rollio: episode format mcap has no resume probe yet; starting at episode_index 0"
            );
            Ok(None)
        }
    }
}

fn read_info_embedded_config_toml(
    root: &Path,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let path = root.join("meta/info.json");
    if !path.is_file() {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(&fs::read(&path)?)?;
    let Some(raw) = value.get("embedded_config_toml").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    Ok(Some(raw.to_string()))
}

fn embedded_configs_match(stored: &str, current: &str) -> bool {
    normalize_embedded_toml(stored) == normalize_embedded_toml(current)
}

fn normalize_embedded_toml(s: &str) -> String {
    s.trim().replace("\r\n", "\n")
}

fn lerobot_resume_hint_if_episodes_exist(
    root: &Path,
) -> Result<Option<DatasetResumeHint>, Box<dyn std::error::Error>> {
    let episodes_path = root.join("meta/episodes.jsonl");
    let (mut max_index, jsonl_count) = max_episode_and_line_count_from_jsonl(&episodes_path)?;
    let (parquet_max, parquet_distinct) = scan_data_parquet_episodes(root)?;
    if let Some(pm) = parquet_max {
        max_index = Some(max_index.map_or(pm, |m| m.max(pm)));
    }

    let Some(max_idx) = max_index else {
        return Ok(None);
    };

    let prior_count = jsonl_count.unwrap_or(parquet_distinct);

    Ok(Some(DatasetResumeHint {
        next_episode_index: max_idx.saturating_add(1),
        prior_stored_episode_count: prior_count,
    }))
}

fn resolve_storage_root(config: &ProjectConfig, invocation_cwd: &Path) -> Option<PathBuf> {
    let path = config.storage.output_path.as_deref()?;
    if path.trim().is_empty() {
        return None;
    }
    let p = Path::new(path);
    Some(if p.is_absolute() {
        p.to_path_buf()
    } else {
        invocation_cwd.join(p)
    })
}

fn max_episode_and_line_count_from_jsonl(
    path: &Path,
) -> Result<(Option<u32>, Option<u32>), Box<dyn std::error::Error>> {
    if !path.is_file() {
        return Ok((None, None));
    }
    let text = fs::read_to_string(path)?;
    let mut max_index: Option<u32> = None;
    let mut valid_lines: u32 = 0;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            eprintln!("rollio: skipping invalid JSON line in {}", path.display());
            continue;
        };
        let Some(n) = value
            .get("episode_index")
            .and_then(|v| v.as_u64().or_else(|| v.as_i64().map(|i| i as u64)))
        else {
            continue;
        };
        let idx = u32::try_from(n).unwrap_or(u32::MAX);
        max_index = Some(max_index.map_or(idx, |m| m.max(idx)));
        valid_lines = valid_lines.saturating_add(1);
    }
    Ok((max_index, Some(valid_lines)))
}

/// Returns `(max_episode_index, distinct_episode_count)` for `data/chunk-*/episode_*.parquet`.
fn scan_data_parquet_episodes(root: &Path) -> Result<(Option<u32>, u32), std::io::Error> {
    let data_root = root.join("data");
    if !data_root.is_dir() {
        return Ok((None, 0));
    }
    let mut seen = HashSet::new();
    for chunk in fs::read_dir(&data_root)? {
        let chunk = chunk?;
        if !chunk.file_type()?.is_dir() {
            continue;
        }
        for ep in fs::read_dir(chunk.path())? {
            let ep = ep?;
            if !ep.file_type()?.is_file() {
                continue;
            }
            if let Some(idx) = parse_episode_parquet_name(&ep.file_name().to_string_lossy()) {
                seen.insert(idx);
            }
        }
    }
    let max = seen.iter().copied().max();
    let n = u32::try_from(seen.len()).unwrap_or(u32::MAX);
    Ok((max, n))
}

fn parse_episode_parquet_name(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("episode_")?;
    let num = rest.strip_suffix(".parquet")?;
    num.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn lerobot_resume_uses_max_episode_from_jsonl_and_parquet(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();
        fs::create_dir_all(root.join("meta"))?;
        let mut ep = std::fs::File::create(root.join("meta/episodes.jsonl"))?;
        writeln!(ep, r#"{{"episode_index":0,"length":1}}"#)?;
        writeln!(ep, r#"{{"episode_index":2,"length":1}}"#)?;

        fs::create_dir_all(root.join("data/chunk-000"))?;
        std::fs::File::create(root.join("data/chunk-000/episode_000004.parquet"))?;

        let hint = lerobot_resume_hint_if_episodes_exist(root)?.expect("episodes exist");
        assert_eq!(
            hint,
            DatasetResumeHint {
                next_episode_index: 5,
                prior_stored_episode_count: 2,
            }
        );
        Ok(())
    }

    #[test]
    fn lerobot_resume_parquet_only_counts_distinct_indices(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();
        fs::create_dir_all(root.join("data/chunk-000"))?;
        std::fs::File::create(root.join("data/chunk-000/episode_000001.parquet"))?;

        let hint = lerobot_resume_hint_if_episodes_exist(root)?.expect("episodes exist");
        assert_eq!(
            hint,
            DatasetResumeHint {
                next_episode_index: 2,
                prior_stored_episode_count: 1,
            }
        );
        Ok(())
    }

    #[test]
    fn lerobot_empty_dataset_yields_no_resume_hint() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();
        fs::create_dir_all(root.join("meta"))?;
        assert!(lerobot_resume_hint_if_episodes_exist(root)?.is_none());
        Ok(())
    }

    #[test]
    fn embedded_configs_match_ignores_cr_lf() {
        assert!(embedded_configs_match("a = 1\n", "a = 1\r\n"));
    }

    #[test]
    fn probe_resume_rejects_mismatched_embedded_config() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("output");
        fs::create_dir_all(root.join("meta"))?;
        fs::create_dir_all(root.join("data/chunk-000"))?;
        std::fs::File::create(root.join("data/chunk-000/episode_000000.parquet"))?;
        fs::write(
            root.join("meta/info.json"),
            r#"{"embedded_config_toml":"project_name = \"old\"\n"}"#,
        )?;

        let cfg = include_str!("../../config/config.example.toml").parse::<ProjectConfig>()?;
        let err = probe_resume(&cfg, tmp.path()).expect_err("mismatch should error");
        assert!(
            err.to_string().contains("different project config"),
            "unexpected: {err}"
        );
        Ok(())
    }

    #[test]
    fn probe_resume_accepts_matching_embedded_config() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("output");
        fs::create_dir_all(root.join("meta"))?;
        fs::create_dir_all(root.join("data/chunk-000"))?;
        std::fs::File::create(root.join("data/chunk-000/episode_000000.parquet"))?;

        let cfg = include_str!("../../config/config.example.toml").parse::<ProjectConfig>()?;
        let toml = toml::to_string(&cfg)?;
        let info = format!(
            "{{\"embedded_config_toml\":{}}}",
            serde_json::to_string(&toml)?
        );
        fs::write(root.join("meta/info.json"), info)?;

        let hint = probe_resume(&cfg, tmp.path())?.expect("should resume");
        assert_eq!(hint.next_episode_index, 1);
        Ok(())
    }
}
