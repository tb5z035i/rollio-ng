use clap::{Parser, ValueHint};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Parser)]
#[command(name = "rollio-test-robot-keyboard")]
#[command(about = "Keyboard robot controller over the Rollio iceoryx2 bus")]
pub struct Args {
    /// TOML config file describing the robots to monitor/control.
    #[arg(short = 'c', long, value_name = "PATH", value_hint = ValueHint::FilePath)]
    pub config: PathBuf,

    /// Frequency for re-publishing the active robot command.
    #[arg(long, default_value_t = 30.0)]
    pub command_rate_hz: f64,

    /// Frequency for redrawing the terminal dashboard.
    #[arg(long, default_value_t = 20.0)]
    pub render_rate_hz: f64,

    /// Joint jog step for normal adjustments.
    #[arg(long, default_value_t = 0.05)]
    pub small_step: f64,

    /// Joint jog step for coarse adjustments.
    #[arg(long, default_value_t = 0.25)]
    pub large_step: f64,
}

impl Args {
    pub fn command_period(&self) -> Result<Duration, String> {
        duration_from_rate("command_rate_hz", self.command_rate_hz)
    }

    pub fn render_period(&self) -> Result<Duration, String> {
        duration_from_rate("render_rate_hz", self.render_rate_hz)
    }

    pub fn validate_steps(&self) -> Result<(), String> {
        validate_step("small_step", self.small_step)?;
        validate_step("large_step", self.large_step)?;
        Ok(())
    }
}

fn duration_from_rate(field: &str, hz: f64) -> Result<Duration, String> {
    if !hz.is_finite() || hz <= 0.0 {
        return Err(format!(
            "{field} must be a positive finite number, got {hz}"
        ));
    }

    Ok(Duration::from_secs_f64(1.0 / hz))
}

fn validate_step(field: &str, value: f64) -> Result<(), String> {
    if !value.is_finite() || value <= 0.0 {
        return Err(format!(
            "{field} must be a positive finite number, got {value}"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Args;

    fn valid_args() -> Args {
        Args {
            config: "config/example.toml".into(),
            command_rate_hz: 20.0,
            render_rate_hz: 10.0,
            small_step: 0.05,
            large_step: 0.2,
        }
    }

    #[test]
    fn rejects_non_positive_rates() {
        let mut args = valid_args();
        args.command_rate_hz = 0.0;
        assert!(args.command_period().is_err());

        args.command_rate_hz = 20.0;
        args.render_rate_hz = -1.0;
        assert!(args.render_period().is_err());
    }

    #[test]
    fn rejects_non_positive_steps() {
        let mut args = valid_args();
        args.small_step = -0.1;
        assert!(args.validate_steps().is_err());

        args.small_step = 0.1;
        args.large_step = 0.0;
        assert!(args.validate_steps().is_err());
    }
}
