use std::io::Read;

use eyre::{eyre, Result};

use ltk_mimir_cache::{Fetch, HashStore, HttpFetchError, UpdateOptions, UpdateOutcome};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::style::ProgressStyle;

/// GitHub `owner/repo` whose latest release ships the mimir `.lhdb` tables.
const MIMIR_REPO: &str = "LeagueToolkit/mimir";

/// `User-Agent` sent with our download requests.
const USER_AGENT: &str = concat!("wadtools/", env!("CARGO_PKG_VERSION"));

/// Progress-bar template for a single `.lhdb` download: filename on its own line,
/// then a byte bar with transfer rate and ETA.
const DOWNLOAD_BAR_TEMPLATE: &str =
    "{msg}\n{wide_bar:40.cyan/blue} {bytes}/{total_bytes} ({bytes_per_sec}, {eta})";

pub struct DownloadHashesArgs {
    /// Optional override for the mimir cache directory. When `None`, the platform
    /// default (or the `MIMIR_DIR` environment variable) is used.
    pub hashtable_dir: Option<String>,
}

/// Downloads/updates the mimir hash tables into the shared cache.
pub fn download_hashes(args: DownloadHashesArgs) -> Result<()> {
    let store = match args.hashtable_dir {
        Some(dir) => HashStore::at(dir),
        None => HashStore::discover()
            .map_err(|e| eyre!("could not determine the mimir cache directory: {e}"))?,
    };

    tracing::info!("updating hash tables in {}", store.dir().display());

    let fetch = ProgressFetch::github(MIMIR_REPO);

    match store.update(&fetch, UpdateOptions::default())? {
        UpdateOutcome::Locked => {
            tracing::info!("another process is already updating the hash tables; nothing to do");
        }
        UpdateOutcome::Completed(report) => {
            if report.is_up_to_date() {
                tracing::info!("hash tables are already up to date");
            } else {
                for table in &report.installed {
                    tracing::info!("installed table '{}'", table.id());
                }
                tracing::info!(
                    "updated {} hash table(s) in {}",
                    report.installed.len(),
                    store.dir().display()
                );
            }
            for unknown in &report.unknown_tables {
                tracing::warn!("skipped unknown table '{unknown}' from the remote manifest");
            }
        }
    }

    Ok(())
}

/// A blocking [`Fetch`] over `ureq` that streams each `.lhdb` asset through an
/// `indicatif` progress bar. mimir's bundled `UreqFetch` buffers the whole
/// response, so it can't drive a byte-level bar.
struct ProgressFetch {
    base_url: String,
    agent: ureq::Agent,
}

impl ProgressFetch {
    fn github(owner_repo: &str) -> Self {
        let agent = ureq::AgentBuilder::new().user_agent(USER_AGENT).build();

        Self {
            base_url: format!("https://github.com/{owner_repo}/releases/latest/download"),
            agent,
        }
    }
}

impl Fetch for ProgressFetch {
    type Error = HttpFetchError;

    fn fetch(&self, filename: &str) -> Result<Vec<u8>, HttpFetchError> {
        let url = format!("{}/{filename}", self.base_url);

        let response = match self.agent.get(&url).call() {
            Ok(response) => response,
            Err(ureq::Error::Status(status, _)) => {
                return Err(HttpFetchError::Status { status, url })
            }
            Err(err) => {
                return Err(HttpFetchError::Transport {
                    url,
                    source: Box::new(err),
                })
            }
        };

        let total: Option<u64> = response
            .header("Content-Length")
            .and_then(|value| value.parse().ok());

        // Only sizeable `.lhdb` tables with a known length get a bar.
        let bar_span = match (filename.ends_with(".lhdb"), total) {
            (true, Some(total)) => Some((tracing::info_span!("download"), total)),
            _ => None,
        };
        let _entered = bar_span.as_ref().map(|(span, _)| span.enter());
        if let Some((span, total)) = &bar_span {
            span.pb_set_style(
                &ProgressStyle::with_template(DOWNLOAD_BAR_TEMPLATE)
                    .unwrap()
                    .progress_chars("#>-"),
            );
            span.pb_set_length(*total);
            span.pb_set_message(filename);
        }

        let mut reader = response.into_reader();
        let mut bytes = Vec::with_capacity(total.unwrap_or(0) as usize);
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|err| HttpFetchError::Transport {
                    url: url.clone(),
                    source: Box::new(err),
                })?;
            if read == 0 {
                break;
            }

            bytes.extend_from_slice(&buffer[..read]);
            if let Some((span, _)) = &bar_span {
                span.pb_set_position(bytes.len() as u64);
            }
        }

        Ok(bytes)
    }
}
