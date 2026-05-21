//! Output rendering — text / JSON-Lines / summary stats. Multiple formats can
//! be enabled at the same time; each is rendered independently.

use std::collections::HashMap;
use std::io::Write;

use anyhow::Result;
use clap::ValueEnum;

use crate::FileReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum)]
pub enum Format {
    Text,
    Json,
    Summary,
}

pub fn render<W: Write>(
    out: &mut W,
    reports: &[FileReport],
    formats: &[Format],
) -> Result<()> {
    for (i, fmt) in formats.iter().enumerate() {
        if i > 0 {
            writeln!(out)?;
        }
        match fmt {
            Format::Text => render_text(out, reports)?,
            Format::Json => render_json(out, reports)?,
            Format::Summary => render_summary(out, reports)?,
        }
    }
    Ok(())
}

fn render_text<W: Write>(out: &mut W, reports: &[FileReport]) -> Result<()> {
    for r in reports {
        if let Some(err) = &r.error {
            writeln!(
                out,
                "ERR: {} (spec [{}]): {}",
                r.path.display(),
                r.spec,
                err
            )?;
            continue;
        }
        if r.ok {
            writeln!(out, "OK: spec [{}] passed on {}", r.spec, r.path.display())?;
        } else {
            writeln!(
                out,
                "spec [{}] failed validation ({} issues):",
                r.spec,
                r.issues.len(),
            )?;
            for e in &r.issues {
                writeln!(out, "  - {}", e)?;
            }
        }
    }
    Ok(())
}

fn render_json<W: Write>(out: &mut W, reports: &[FileReport]) -> Result<()> {
    for r in reports {
        let line = serde_json::to_string(r)?;
        writeln!(out, "{}", line)?;
    }
    Ok(())
}

fn render_summary<W: Write>(out: &mut W, reports: &[FileReport]) -> Result<()> {
    let total = reports.len();
    let passed = reports.iter().filter(|r| r.ok).count();
    let failed = total - passed;
    let errored = reports.iter().filter(|r| r.error.is_some()).count();

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for r in reports {
        for issue in &r.issues {
            *counts.entry(issue.as_str()).or_default() += 1;
        }
    }
    let mut top: Vec<(&&str, &usize)> = counts.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));

    writeln!(out, "=== summary ===")?;
    writeln!(out, "total:   {}", total)?;
    writeln!(out, "passed:  {}", passed)?;
    writeln!(out, "failed:  {}", failed)?;
    if errored > 0 {
        writeln!(out, "errored: {}", errored)?;
    }
    if !top.is_empty() {
        writeln!(out, "top failure reasons:")?;
        for (msg, count) in top.iter().take(10) {
            writeln!(out, "  {:>5}x  {}", count, msg)?;
        }
    }
    Ok(())
}
