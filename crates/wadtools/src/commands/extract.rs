use camino::{Utf8Path, Utf8PathBuf};
use color_eyre::owo_colors::OwoColorize;
use convert_case::{Case, Casing};
use fancy_regex::Regex;
use std::fs::File;

use league_toolkit::{
    file::LeagueFileKind,
    wad::{ExistingFilePolicy, ExtractReport, Wad, WadExtractor, WadHash},
};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::style::ProgressStyle;

use crate::{
    filters::{should_skip_hash, should_skip_pattern, should_skip_type},
    utils::{create_filter_pattern, format_size, truncate_middle, WadHashtable},
};

const MAX_LOG_PATH_LEN: usize = 120;

pub struct ExtractArgs {
    pub input: String,
    pub output: Option<String>,
    pub filter_type: Option<Vec<LeagueFileKind>>,
    pub pattern: Option<String>,
    pub hash: Option<Vec<WadHash>>,
    pub filter_invert: bool,
    pub overwrite: bool,
    pub show_stats: bool,
    pub resolve_bin_paths: bool,
}

pub fn extract(mut args: ExtractArgs, hashtable: &WadHashtable) -> eyre::Result<()> {
    let source = File::open(&args.input)?;
    let mut wad = Wad::mount(source)?;

    let filter_pattern = create_filter_pattern(args.pattern.take())?;
    let output_dir = resolve_output_dir(&args.input, args.output.as_deref());

    let report = run(
        &mut wad,
        hashtable,
        &args,
        filter_pattern.as_ref(),
        &output_dir,
    )?;

    if !report.recovered.is_empty() {
        tracing::info!(
            "recovered {} chunk name(s) from bin files",
            report.recovered.len()
        );
    }

    if args.show_stats {
        print_stats(&args.input, &report);
    } else if report.skipped_existing > 0 {
        tracing::info!(
            "extracted {} chunks, skipped {} existing :)",
            report.extracted,
            report.skipped_existing
        );
    } else {
        tracing::info!("extracted {} chunks :)", report.extracted);
    }

    Ok(())
}

/// Runs the extraction under its progress span, so the bar is gone before the
/// summary prints.
fn run(
    wad: &mut Wad<File>,
    hashtable: &WadHashtable,
    args: &ExtractArgs,
    filter_pattern: Option<&Regex>,
    output_dir: &Utf8Path,
) -> eyre::Result<ExtractReport> {
    let filter_invert = args.filter_invert;

    // The hash filter is the cheapest check, so it picks the chunks before anything is read.
    let selected: Vec<WadHash> = wad
        .chunks()
        .iter()
        .map(|chunk| chunk.path_hash)
        .filter(|&path_hash| !should_skip_hash(path_hash, args.hash.as_deref(), filter_invert))
        .collect();

    let span = tracing::info_span!("extract", total = selected.len());
    let _entered = span.enter();
    span.pb_set_style(
        &ProgressStyle::with_template("{wide_bar:40.cyan/blue} {pos}/{len} \n {spinner} {msg}")
            .unwrap(),
    );
    span.pb_set_length(selected.len() as u64);
    span.pb_set_message("Extracting chunks");

    let existing = if args.overwrite {
        ExistingFilePolicy::Overwrite
    } else {
        ExistingFilePolicy::Skip
    };
    let mut extractor = WadExtractor::new(hashtable)
        .with_existing_file_policy(existing)
        .on_progress(|progress| {
            span.pb_set_message(&truncate_middle(progress.path(), MAX_LOG_PATH_LEN));
            span.pb_set_position(progress.done() as u64);
        });
    if let Some(pattern) = filter_pattern {
        extractor = extractor
            .with_filter(move |path| !should_skip_pattern(path, Some(pattern), filter_invert));
    }
    if let Some(kinds) = args.filter_type.as_deref() {
        // The crate's type filter only keeps, so `-v` hands it the complement.
        let kept = LeagueFileKind::iter()
            .filter(|&kind| !should_skip_type(kind, Some(kinds), filter_invert));
        extractor = extractor.with_type_filter(kept);
    }
    if args.resolve_bin_paths {
        extractor = extractor.with_name_recovery();
    }

    Ok(extractor.extract_chunks(wad, selected, output_dir)?)
}

/// The output directory, or a sibling directory named after the input file
/// without its extension.
fn resolve_output_dir(input: &str, output: Option<&str>) -> Utf8PathBuf {
    match output {
        Some(path) => Utf8PathBuf::from(path),
        None => {
            let input_path = Utf8Path::new(input);
            let parent = input_path.parent().unwrap_or(Utf8Path::new("."));
            let stem = input_path.file_stem().unwrap_or("extracted");
            parent.join(stem)
        }
    }
}

fn print_stats(input: &str, report: &ExtractReport) {
    println!();
    println!("{}: {}", "WAD".bright_cyan().bold(), input.bright_white());
    println!(
        "{}: {} chunks ({})",
        "Extracted".bright_cyan().bold(),
        report.extracted.to_string().bright_green(),
        format_size(report.bytes_written).bright_white()
    );
    println!(
        "{}: {} existing",
        "Skipped".bright_cyan().bold(),
        report.skipped_existing.to_string().bright_yellow()
    );
    if !report.recovered.is_empty() {
        println!(
            "{}: {} names from bins",
            "Recovered".bright_cyan().bold(),
            report.recovered.len().to_string().bright_green()
        );
    }
    if !report.by_kind.is_empty() {
        println!();
        println!("{}:", "By type".bright_cyan().bold());
        let mut type_entries: Vec<_> = report.by_kind.iter().collect();
        type_entries.sort_by(|a, b| b.1.cmp(a.1));
        for (kind, count) in type_entries {
            let name = format!("{:?}", kind).to_case(Case::Snake);
            println!(
                "  {:24} {}",
                name.bright_magenta(),
                count.to_string().bright_white()
            );
        }
    }
}

pub fn print_supported_filters() {
    println!("Supported filter types (name -> description [extension]):");
    for kind in LeagueFileKind::iter().collect::<Vec<_>>() {
        let ext = kind.extension().unwrap_or("");
        let snake = format!("{:?}", kind).to_case(Case::Snake);
        println!(
            "  {:24} -> {:?} [{}]",
            snake.bright_yellow().bold(),
            kind,
            ext.bright_green().bold()
        );
    }
}
