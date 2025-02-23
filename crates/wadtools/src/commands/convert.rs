use std::{io::BufReader, path::PathBuf};

use clap::{Args, ValueEnum};
use eyre::bail;

#[derive(ValueEnum, Debug, Clone)]
pub enum OutputFormat {
    Png,
}

#[derive(Args, Debug, Clone)]
pub struct ConvertArgs {
    /// Path to the input file
    pub input: PathBuf,
    /// Desired output format
    #[arg(long)]
    pub to: OutputFormat,
}

pub fn convert(args: ConvertArgs) -> eyre::Result<()> {
    let file = std::fs::File::open(&args.input).map(BufReader::new)?;
    let Some(ext) = args.input.extension() else {
        bail!("detecting file by magic not yet supported!");
    };

    Ok(())
}
