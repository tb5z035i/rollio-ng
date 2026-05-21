use clap::{Args, Parser, Subcommand, ValueEnum};
use rollio_types::config::ProjectConfig;
use std::error::Error;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum SetupBackend {
    /// Ink/React terminal UI spawned via `node`. Requires Node.js on PATH.
    #[default]
    Tui,
    /// `rollio-web-gateway` serving the browser SPA. No Node.js needed at runtime.
    Web,
}

#[derive(Debug, Parser)]
#[command(name = "rollio")]
#[command(about = "Rollio orchestration CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Collect(CollectArgs),
    Setup(SetupArgs),
}

#[derive(Debug, Clone, Args)]
pub struct CollectArgs {
    #[arg(
        short = 'c',
        long = "config",
        value_name = "PATH",
        conflicts_with = "config_inline"
    )]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    pub config_inline: Option<String>,
}

impl CollectArgs {
    pub fn load_project_config(&self) -> Result<ProjectConfig, Box<dyn Error>> {
        if let Some(config_path) = &self.config {
            return Ok(ProjectConfig::from_file(config_path)?);
        }
        if let Some(config_inline) = &self.config_inline {
            return Ok(config_inline.parse::<ProjectConfig>()?);
        }

        Err("collect requires either --config or --config-inline".into())
    }
}

#[derive(Debug, Clone, Args)]
pub struct SetupArgs {
    #[arg(
        short = 'c',
        long = "config",
        value_name = "PATH",
        conflicts_with = "config_inline"
    )]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    pub config_inline: Option<String>,
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub accept_defaults: bool,
    /// Inject simulated `rollio-device-pseudo` devices during discovery.
    /// Replaces the legacy `--sim-cameras` / `--sim-arms` split: the pseudo
    /// driver itself decides what mix of camera/robot channels to emit
    /// based on its `--count` arg.
    #[arg(long = "sim-pseudo", default_value_t = 0)]
    pub sim_pseudo: usize,
    /// Which interactive UI to run. `tui` (default) keeps the Ink terminal
    /// wizard; `web` launches `rollio-web-gateway` and opens the browser.
    #[arg(long, value_enum, default_value_t = SetupBackend::Tui)]
    pub backend: SetupBackend,
    /// With `--backend web`, print the gateway URL but don't auto-open the
    /// browser. Useful over SSH and in headless CI. Ignored under `tui`.
    #[arg(long, default_value_t = false)]
    pub no_open: bool,
}

impl SetupArgs {
    pub fn load_project_config(&self) -> Result<Option<ProjectConfig>, Box<dyn Error>> {
        if let Some(config_path) = &self.config {
            return Ok(Some(ProjectConfig::from_file(config_path)?));
        }
        if let Some(config_inline) = &self.config_inline {
            return Ok(Some(config_inline.parse::<ProjectConfig>()?));
        }
        Ok(None)
    }

    pub fn output_path(&self) -> PathBuf {
        self.output
            .clone()
            .unwrap_or_else(|| PathBuf::from("config.toml"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse_setup(args: &[&str]) -> SetupArgs {
        let cli = Cli::try_parse_from(
            std::iter::once("rollio")
                .chain(std::iter::once("setup"))
                .chain(args.iter().copied()),
        )
        .expect("setup args parse");
        match cli.command {
            Command::Setup(setup) => setup,
            other => panic!("expected Setup command, got {other:?}"),
        }
    }

    #[test]
    fn setup_defaults_to_tui_backend_with_no_open_false() {
        let args = parse_setup(&[]);
        assert_eq!(args.backend, SetupBackend::Tui);
        assert!(!args.no_open);
    }

    #[test]
    fn setup_accepts_backend_web() {
        let args = parse_setup(&["--backend", "web"]);
        assert_eq!(args.backend, SetupBackend::Web);
    }

    #[test]
    fn setup_no_open_flag_parses_with_web_backend() {
        let args = parse_setup(&["--backend", "web", "--no-open"]);
        assert_eq!(args.backend, SetupBackend::Web);
        assert!(args.no_open);
    }

    #[test]
    fn setup_rejects_unknown_backend_value() {
        let err = Cli::try_parse_from(["rollio", "setup", "--backend", "gui"]).unwrap_err();
        assert!(err.to_string().contains("backend"));
    }
}
