# wadtools

[![CI](https://github.com/LeagueToolkit/wadtools/actions/workflows/ci.yml/badge.svg)](https://github.com/LeagueToolkit/wadtools/actions/workflows/ci.yml)
[![Release](https://github.com/LeagueToolkit/wadtools/actions/workflows/release.yml/badge.svg)](https://github.com/LeagueToolkit/wadtools/actions/workflows/release.yml)
[![License: GPL-3.0](https://img.shields.io/badge/License-GPL%203.0-blue.svg)](https://opensource.org/licenses/GPL-3.0)

Tooling for interacting with `.wad` files. This command-line utility provides a set of tools for working with `.wad` archive files found in League of Legends.

## Features

- **Extract**: Extract contents from WAD files
- **List**: Browse WAD file contents without extracting
- **Diff**: Compare WAD files and show differences

## Installation

### Windows (Quick Install)

Run this in PowerShell (uses a default user-writable directory and updates PATH):

```powershell
irm https://raw.githubusercontent.com/LeagueToolkit/wadtools/main/scripts/install-wadtools.ps1 | iex
```

Advanced (choose a custom directory):

```powershell
# Download and run with parameters
$tmp = Join-Path $env:TEMP 'install-wadtools.ps1'
iwr -useb https://raw.githubusercontent.com/LeagueToolkit/wadtools/main/scripts/install-wadtools.ps1 -OutFile $tmp
powershell -ExecutionPolicy Bypass -File $tmp -InstallDir "$env:LOCALAPPDATA\wadtools\bin"
Remove-Item $tmp -Force
```

### From Releases

Download the latest release for your platform from the [Releases page](https://github.com/LeagueToolkit/wadtools/releases).

Available binaries:

- Windows (x64): `wadtools-windows.exe`
- Linux (x64): `wadtools-linux`
- macOS (x64): `wadtools-macos`

### From Source

To build from source, you'll need:

- Rust (nightly toolchain)
- Cargo (Rust's package manager)

```bash
# Clone the repository
git clone https://github.com/LeagueToolkit/wadtools.git
cd wadtools

# Build the project
cargo build --release

# The binary will be available in target/release/
```

## Usage

```bash
# Basic command structure
wadtools <COMMAND> [OPTIONS]

# Show command help
wadtools --help
wadtools <COMMAND> --help
```

Global options:

- `-L, --verbosity <LEVEL>`: set log verbosity (`error`, `warning`, `info`, `debug`, `trace`)
- `--config <FILE>`: load options from a TOML file (defaults to `wadtools.toml` next to the executable; created on first run)
- `--progress <true|false>`: show/hide progress bars (overrides config)
- `--hashtable-dir <DIR>`: recursively load hashtable files from this directory (overrides defaults and config)

### Extract

Extracts files from a WAD archive. Use `-i/--input` for the WAD file, `-o/--output` for the destination directory.

Common flags:

- `-i, --input <PATH>`: path to the input WAD file
- `-o, --output <DIR>`: output directory
- `-H, --hashtable <PATH>` (also `-d`): optional hashtable file to resolve names
- `-f, --filter-type <TYPE...>`: filter by file type(s) like `png`, `tga`, `bin`
- `-x, --pattern <REGEX>`: filter by regex on the resolved path (see below)

Basic examples:

```bash
# Extract everything (recommended to provide a hashtable)
wadtools extract -i Aatrox.wad.client -o out -H hashes.game.txt

# Extract only textures (DDS or TEX) under assets/
wadtools extract -i Aatrox.wad.client -o out -H hashes.game.txt \
  -f dds tex -x "^assets/.*\.(dds|tex)$"
```

Configuration file example (`wadtools.toml`):

```toml
# Show progress bars by default (can be overridden by CLI)
show_progress = true

# Optional custom directory where hashtable files are loaded from
# If set, wadtools will recursively load all files in this directory on start
# This can be overridden by the CLI flag --hashtable-dir
hashtable_dir = "C:/Users/you/Documents/LeagueToolkit/wad_hashtables"
```

### Defaults: config and hashtable discovery

- **Config file**:

  - By default we create and read `wadtools.toml` next to the executable, regardless of current directory.
  - You can point to a different file via `--config <FILE>`.
  - Precedence: CLI flags override config. `--progress=true|false` persists back into the resolved config file.

- **Hashtable files**:
  - We load hashtables recursively from one of the following, in order:
    1. `--hashtable-dir <DIR>` if provided
    2. `hashtable_dir` from `wadtools.toml` if set
    3. Default directory:
       - On Windows: `Documents/LeagueToolkit/wad_hashtables`.
       - On other platforms: platform data dir from `directories_next` under `io/LeagueToolkit/wadtools/wad_hashtables`.
  - If `-H/--hashtable <PATH>` is provided, that specific file is also loaded in addition to the directory above.
  - If none of the directories exist, only the provided file (if any) is loaded.

How filtering works:

- `--pattern/-x` and `--filter-type/-f` are combined with AND semantics.
  - A chunk must match the regex AND be one of the selected types to be extracted if both flags are provided.
- Regex is case-insensitive by default.
  - To opt out, prefix the pattern with `(?-i)`.
  - Backreferences and lookarounds are supported.

Regex examples:

```bash
# Case-insensitive (default)
wadtools extract -i Aatrox.wad.client -o out -H hashes.game.txt \
  -x "^assets/.*\.(png|tga)$"

# Backreference example: DATA/Characters/<name>/<name>.bin
wadtools extract -i Aatrox.wad.client -o out -H hashes.game.txt \
  -x "(?i)^DATA/Characters/(.*?)/\\1\\.bin$"
```

Name resolution with hashtable:

- Without a hashtable, unknown paths are written using their 16-character hex hash (e.g., `2f3c...b9a`).
- With `-H/--hashtable`, matching hashes are resolved to readable paths before extraction.

When we add the `.ltk` postfix:

- We append `.ltk` if the original path has no extension or the resolved destination would collide with an existing directory (this happens for a lot of `.bin` files in `UI.wad.client` for example).
- If we can detect the real type from file contents, we append it after `.ltk`, e.g. `foo.ltk.png`; otherwise just `foo.ltk`.

Handling long filenames:

- If the platform/filesystem rejects a write due to a long filename, we fall back to the chunk hash as the filename (16 hex chars) in the output directory.
- A warning is logged including both the readable path (if known) and the hashed path so you can correlate outputs.

File type filtering (`-f/--filter-type`):

- Uses content detection to identify types like `png`, `tga`, `bin`, etc.
- You can pass multiple values: `-f png tga`.
- Remember this ANDs with `--pattern` when both are provided.

### List

Lists all chunks in a WAD file with metadata. Use `-i/--input` for the WAD file. Alias: `ls`.

Common flags:

- `-i, --input <PATH>`: path to the input WAD file
- `-H, --hashtable <PATH>` (also `-d`): optional hashtable file to resolve names
- `-f, --filter-type <TYPE...>`: filter by file type(s) like `png`, `bin`, `dds`
- `-x, --pattern <REGEX>`: filter by regex on the resolved path
- `-F, --format <FORMAT>`: output format (`table`, `json`, `csv`, `flat`)
- `-s, --stats`: show summary statistics (default: true)

Basic examples:

```bash
# List all files in a WAD with a nice table view
wadtools list -i Aatrox.wad.client
wadtools ls -i Aatrox.wad.client  # using alias

# List only texture files
wadtools ls -i Aatrox.wad.client -f dds png tex

# Search for specific files using regex
wadtools ls -i Aatrox.wad.client -x "data/.*\.bin$"

# Export file list as JSON for scripting
wadtools ls -i Aatrox.wad.client -F json > files.json

# Export as CSV for spreadsheets
wadtools ls -i Aatrox.wad.client -F csv > files.csv

# Get just file paths (great for piping)
wadtools ls -i Aatrox.wad.client -F flat | grep "\.png$"
```

Output formats:

- `table` (default): colored table with compressed/uncompressed sizes, compression ratio, and file types
- `json`: structured JSON with full metadata
- `csv`: spreadsheet-friendly format
- `flat`: plain list of paths only, one per line

### Diff

Compares two WAD files and shows differences.

Quick example:

```bash
wadtools diff -r old.wad.client -t new.wad.client -H hashtable.txt \
  -o diff.csv
```

### Hashtable Directory

Show the default hashtable directory:

```bash
wadtools hashtable-dir
# or
wadtools hd
```

## Development

1. Install development tools:

   ```bash
   rustup component add rustfmt clippy
   ```

2. Run tests:

   ```bash
   cargo test
   ```

3. Check formatting:

   ```bash
   cargo fmt --all -- --check
   ```

4. Run clippy:
   ```bash
   cargo clippy -- -D warnings
   ```

## License

This project is licensed under the GNU General Public License v3.0 - see the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request. For major changes, please open an issue first to discuss what you would like to change.

Please make sure to update tests as appropriate and follow the existing code style.

## Acknowledgments

- Thanks to all contributors who have helped with the development of this tool
- Built using the [league-toolkit](https://github.com/league-toolkit) library
