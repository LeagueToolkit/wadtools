# wadtools

[![CI](https://github.com/LeagueToolkit/wadtools/actions/workflows/ci.yml/badge.svg)](https://github.com/LeagueToolkit/wadtools/actions/workflows/ci.yml)
[![Release](https://github.com/LeagueToolkit/wadtools/actions/workflows/release.yml/badge.svg)](https://github.com/LeagueToolkit/wadtools/actions/workflows/release.yml)
[![License: GPL-3.0](https://img.shields.io/badge/License-GPL%203.0-blue.svg)](https://opensource.org/licenses/GPL-3.0)

Tooling for interacting with `.wad` files. This command-line utility provides a set of tools for working with WAD (Wwise Audio Database) files, commonly used in games.

## Features

- **Extract**: Extract contents from WAD files
- **Diff**: Compare WAD files and show differences
- More features coming soon!

## Installation

### From Releases

Download the latest release for your platform from the [Releases page](https://github.com/LeagueToolkit/wadtools/releases).

Available binaries:
- Windows (x64): `wadtools-windows-amd64.exe`
- Linux (x64): `wadtools-linux-amd64`
- macOS (x64): `wadtools-macos-amd64`

### From Source

To build from source, you'll need:
- Rust (nightly toolchain)
- Cargo (Rust's package manager)

```bash
# Clone the repository
git clone https://github.com/Crauzer/wadtools.git
cd wadtools

# Install nightly toolchain
rustup toolchain install nightly
rustup override set nightly

# Build the project
cargo build --release

# The binary will be available in target/release/
```

## Usage

```bash
# Basic command structure
wadtools <COMMAND> [OPTIONS]

# Extract contents from a WAD file
wadtools extract <WAD_FILE> <OUTPUT_DIR>

# Compare two WAD files
wadtools diff <WAD_FILE_1> <WAD_FILE_2>
```

For detailed usage of each command, use the `--help` flag:
```bash
wadtools --help
wadtools <COMMAND> --help
```

## Development

This project uses Rust's nightly features. To contribute:

1. Ensure you have the nightly toolchain:
   ```bash
   rustup toolchain install nightly
   rustup override set nightly
   ```

2. Install development tools:
   ```bash
   rustup component add rustfmt clippy
   ```

3. Run tests:
   ```bash
   cargo test
   ```

4. Check formatting:
   ```bash
   cargo fmt --all -- --check
   ```

5. Run clippy:
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
