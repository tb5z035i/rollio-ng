use clap::{Args, Parser, Subcommand};
use rollio_types::config::Config;
use rollio_types::schema::build_config_schema;
use std::error::Error;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "rollio-config")]
#[command(about = "Static Rollio config validation and schema export")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Validate(ValidateArgs),
    Schema,
}

#[derive(Debug, Args)]
struct ValidateArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    config_inline: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rollio-config: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate(args) => validate(args)?,
        Command::Schema => println!("{}", serde_json::to_string_pretty(&build_config_schema())?),
    }
    Ok(())
}

fn validate(args: ValidateArgs) -> Result<(), Box<dyn Error>> {
    if let Some(path) = args.config {
        Config::from_file(&path)?;
        println!("config is valid: {}", path.display());
    } else if let Some(inline) = args.config_inline {
        inline.parse::<Config>()?;
        println!("config is valid");
    } else {
        return Err("validate requires either --config or --config-inline".into());
    }
    Ok(())
}
