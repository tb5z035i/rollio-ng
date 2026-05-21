use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;

use mcap_validate::batch::{collect_mcaps, run_batch, BatchOptions};
use mcap_validate::report::{self, Format};
use mcap_validate::spec::Spec;

#[derive(Parser, Debug)]
#[command(
    name = "mcap-validate",
    about = "Validate one or many MCAP files against a per-station spec TOML.",
    long_about = "Validates a single MCAP file or a directory tree of MCAP files against a \
                  spec TOML. Directory mode walks recursively for *.mcap and validates each \
                  file against the same spec, in parallel. See mcap_spec/data_spec_template.toml \
                  for the spec format."
)]
struct Cli {
    /// MCAP file, or directory to walk recursively for *.mcap.
    path: PathBuf,

    /// Per-station spec TOML.
    #[arg(long)]
    spec: PathBuf,

    /// Output format. Pass multiple times to emit several formats.
    #[arg(long, value_enum, default_values_t = [Format::Text])]
    format: Vec<Format>,

    /// Number of worker threads. Defaults to the physical core count.
    #[arg(long)]
    jobs: Option<usize>,

    /// Skip sync_group + tf_pair message-level checks.
    #[arg(long, default_value_t = false)]
    no_constraints: bool,

    /// Stop scheduling new files after the first failure.
    #[arg(long, default_value_t = false)]
    fail_fast: bool,
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {:#}", e);
            ExitCode::from(2)
        }
    }
}

fn real_main() -> Result<ExitCode> {
    let cli = Cli::parse();

    let spec = Arc::new(
        Spec::load(&cli.spec).with_context(|| format!("loading spec {}", cli.spec.display()))?,
    );

    let files = collect_mcaps(&cli.path)?;
    if files.is_empty() {
        eprintln!("no .mcap files found under {}", cli.path.display());
        return Ok(ExitCode::from(1));
    }

    let jobs = cli.jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    });

    let reports = run_batch(
        files,
        Arc::clone(&spec),
        BatchOptions {
            jobs,
            skip_constraints: cli.no_constraints,
            fail_fast: cli.fail_fast,
        },
    )?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    report::render(&mut out, &reports, &cli.format)?;

    let any_failed = reports.iter().any(|r| !r.ok);
    Ok(if any_failed {
        ExitCode::from(1)
    } else {
        ExitCode::from(0)
    })
}
