use camino::{Utf8Path, Utf8PathBuf};
use eyre::{eyre, Result};
use std::fs::{self, File};
use std::io::{Read, Write};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::utils::default_hashtable_dir;

/// Format a path as a clickable hyperlink using OSC 8 escape sequences.
/// Supported by modern terminals like Windows Terminal, iTerm2, VS Code terminal, etc.
fn hyperlink_path(path: &Utf8PathBuf) -> String {
    let url = format!("file://{}", path.as_str().replace('\\', "/"));
    format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url, path)
}

const HASH_FILES: &[(&str, &str)] = &[
    (
        "hashes.game.txt",
        "https://raw.communitydragon.org/binviewer/hashes/hashes.game.txt",
    ),
    (
        "hashes.lcu.txt",
        "https://raw.communitydragon.org/binviewer/hashes/hashes.lcu.txt",
    ),
];

const DOWNLOAD_BUFFER_SIZE: usize = 64 * 1024;

pub struct DownloadHashesArgs {
    pub hashtable_dir: Option<String>,
}

pub fn download_hashes(args: DownloadHashesArgs) -> Result<()> {
    let target_dir = match args.hashtable_dir {
        Some(dir) => Utf8Path::new(&dir).to_path_buf(),
        None => default_hashtable_dir()
            .ok_or_else(|| eyre!("Could not determine default hashtable directory"))?,
    };

    fs::create_dir_all(target_dir.as_std_path())?;

    tracing::info!("Downloading hashtables to {}", hyperlink_path(&target_dir));

    for (filename, url) in HASH_FILES {
        download_file_with_progress(url, filename, &target_dir)?;
    }

    tracing::info!(
        "Successfully downloaded all hashtables to {}",
        hyperlink_path(&target_dir)
    );
    Ok(())
}

fn download_file_with_progress(
    url: &str,
    filename: &str,
    target_dir: &camino::Utf8PathBuf,
) -> Result<()> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| eyre!("Failed to download {}: {}", filename, e))?;

    // Get content length for progress bar (if available)
    let content_length: Option<u64> = response
        .header("Content-Length")
        .and_then(|s| s.parse().ok());

    let target_path = target_dir.join(filename);
    let mut file = File::create(target_path.as_std_path())?;

    let mut reader = response.into_reader();
    let mut buffer = [0u8; DOWNLOAD_BUFFER_SIZE];
    let mut downloaded: u64 = 0;

    // Create a tracing span for the progress bar
    let span = tracing::info_span!("download", file = %filename);
    let _entered = span.enter();

    if let Some(total) = content_length {
        span.pb_set_style(
            &indicatif::ProgressStyle::with_template(
                "{msg} {wide_bar:40.cyan/blue} {bytes}/{total_bytes} ({bytes_per_sec})",
            )
            .unwrap(),
        );
        span.pb_set_length(total);
    } else {
        span.pb_set_style(
            &indicatif::ProgressStyle::with_template("{msg} {bytes} downloaded ({bytes_per_sec})")
                .unwrap(),
        );
    }
    span.pb_set_message(filename);

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        file.write_all(&buffer[..bytes_read])?;
        downloaded += bytes_read as u64;
        span.pb_set_position(downloaded);
    }

    tracing::info!(
        "Saved {} ({} bytes)",
        hyperlink_path(&target_path),
        downloaded
    );
    Ok(())
}
