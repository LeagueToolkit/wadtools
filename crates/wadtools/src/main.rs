use clap::builder::{styling::AnsiColor, Styles};
use clap::error::ErrorKind;
use clap::{Parser, Subcommand, ValueEnum};
use league_toolkit::file::LeagueFileKind;
use std::path::Path;
use tracing::Level;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{filter, fmt};
use utils::config::{default_config_path, load_or_create_config, resolve_and_persist_progress};

mod commands;
mod extractor;
mod utils;

use commands::*;

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum VerbosityLevel {
    /// Show errors and above
    Error,
    /// Show warnings and above
    Warning,
    /// Show info messages and above
    Info,
    /// Show debug messages and above
    Debug,
    /// Show all messages including trace
    Trace,
}

impl From<VerbosityLevel> for Level {
    fn from(level: VerbosityLevel) -> Self {
        match level {
            VerbosityLevel::Error => Level::ERROR,
            VerbosityLevel::Warning => Level::WARN,
            VerbosityLevel::Info => Level::INFO,
            VerbosityLevel::Debug => Level::DEBUG,
            VerbosityLevel::Trace => Level::TRACE,
        }
    }
}

impl VerbosityLevel {
    pub fn to_level_filter(&self) -> LevelFilter {
        LevelFilter::from_level((*self).into())
    }
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None, styles = cli_styles())]
struct Args {
    /// Set the verbosity level
    #[arg(short = 'L', long, value_enum, default_value_t = VerbosityLevel::Info)]
    verbosity: VerbosityLevel,

    /// Optional path to a config file (TOML). Defaults to `wadtools.toml` if present
    #[arg(long)]
    config: Option<String>,

    /// Show or hide progress bars: true/false (overrides config). Example: --progress=false
    #[arg(long, value_name = "true|false")]
    progress: Option<bool>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Extract the contents of a wad file
    #[command(visible_alias = "e")]
    Extract {
        /// Path to the input wad file
        #[arg(short, long)]
        input: String,

        /// Path to the output directory
        #[arg(short, long)]
        output: String,

        /// Path to the hashtable file
        #[arg(short = 'H', long, visible_short_alias = 'd')]
        hashtable: Option<String>,

        #[arg(
            short = 'f',
            long,
            value_name = "FILTER_MAGIC",
            help = "Filter files by magic (e.g., 'png', 'bin'). You can pass multiple values at once.",
            value_parser = parse_filter_type,
            num_args = 1..
        )]
        filter_type: Option<Vec<LeagueFileKind>>,

        /// Only extract chunks whose resolved path matches this regex
        #[arg(
            short = 'x',
            long,
            value_name = "REGEX",
            help = "Only extract chunks whose resolved path matches this regex (case-insensitive by default; use (?-i) to disable)"
        )]
        pattern: Option<String>,
    },
    /// Compare two wad files
    ///
    /// This command compares two wad files and prints the differences between them.
    /// Using the reference wad file, it will print the differences between the target wad file.
    ///
    #[command(visible_alias = "d")]
    Diff {
        /// Path to the reference wad file
        #[arg(short, long)]
        reference: String,

        /// Path to the target wad file
        #[arg(short, long)]
        target: String,

        /// Path to the hashtable file
        #[arg(short = 'H', long, visible_short_alias = 'd')]
        hashtable: Option<String>,

        /// Output the diffs to a .csv file
        #[arg(short, long, help = "The path to the output .csv file")]
        output: Option<String>,
    },
}

fn main() -> eyre::Result<()> {
    let args = match Args::try_parse() {
        Ok(a) => a,
        Err(e) => {
            if matches!(
                e.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion | ErrorKind::MissingSubcommand
            ) {
                // Ensure a default config exists even when showing help/version or missing subcommand
                let _ = load_or_create_config(Some(default_config_path().as_path()));
                e.print()?;
                return Ok(());
            } else {
                e.exit();
            }
        }
    };

    let config_path = args
        .config
        .as_deref()
        .map(Path::new)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(default_config_path);
    let (mut config, resolved_path) = load_or_create_config(Some(config_path.as_path()))?;
    let show_progress =
        resolve_and_persist_progress(&mut config, resolved_path.as_path(), args.progress)?;

    initialize_tracing(args.verbosity, show_progress)?;

    match args.command {
        Commands::Extract {
            input,
            output,
            hashtable,
            filter_type,
            pattern,
        } => extract(ExtractArgs {
            input,
            output,
            hashtable,
            filter_type,
            pattern,
        }),
        Commands::Diff {
            reference,
            target,
            hashtable,
            output,
        } => diff(DiffArgs {
            reference,
            target,
            hashtable_path: hashtable,
            output,
        }),
    }
}

fn initialize_tracing(verbosity: VerbosityLevel, show_progress: bool) -> eyre::Result<()> {
    let indicatif_layer = IndicatifLayer::new();

    let common_format = fmt::format()
        .with_ansi(true)
        .with_level(true)
        .with_source_location(false)
        .with_line_number(false)
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::time());

    // stdout: INFO/DEBUG/TRACE (when verbosity allows)
    let stdout_layer = fmt::layer()
        .with_writer(indicatif_layer.get_stdout_writer())
        .event_format(common_format.clone())
        .with_filter(filter::filter_fn(move |metadata| {
            let level = *metadata.level();
            // Show INFO and above on stdout for Info verbosity and above
            // Show DEBUG and above for Debug verbosity and above
            // Show TRACE for Trace verbosity
            match verbosity {
                VerbosityLevel::Error => {
                    false // Only stderr for this level
                }
                VerbosityLevel::Warning => level == Level::WARN || level == Level::ERROR,
                VerbosityLevel::Info => {
                    level == Level::INFO || level == Level::WARN || level == Level::ERROR
                }
                VerbosityLevel::Debug => {
                    level != Level::TRACE // Everything except TRACE
                }
                VerbosityLevel::Trace => {
                    true // Everything
                }
            }
        }));

    // stderr: WARN/ERROR (for Warning and above) or all high-priority messages
    let stderr_layer = fmt::layer()
        .with_writer(indicatif_layer.get_stderr_writer())
        .event_format(common_format)
        .with_filter(filter::filter_fn(move |metadata| {
            let level = *metadata.level();
            // Show ERROR and WARN on stderr for most verbosity levels
            // For very quiet levels, show only ERROR
            match verbosity {
                VerbosityLevel::Error => level == Level::ERROR,
                VerbosityLevel::Warning => level == Level::WARN || level == Level::ERROR,
                _ => level == Level::WARN || level == Level::ERROR,
            }
        }));

    let registry = tracing_subscriber::registry()
        .with(stdout_layer)
        .with(stderr_layer)
        .with(verbosity.to_level_filter());

    if show_progress {
        registry.with(indicatif_layer).init();
    } else {
        registry.init();
    }
    Ok(())
}

fn parse_filter_type(s: &str) -> Result<LeagueFileKind, String> {
    match LeagueFileKind::from_extension(s) {
        LeagueFileKind::Unknown => Err(format!("Unknown file kind: {}", s)),
        other => Ok(other),
    }
}

fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default().bold())
        .usage(AnsiColor::Green.on_default().bold())
        .literal(AnsiColor::Cyan.on_default())
        .placeholder(AnsiColor::Magenta.on_default())
}
