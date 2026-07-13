# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Hash → path resolution now uses the [mimir](https://github.com/LeagueToolkit/mimir) shared
  `.lhdb` cache instead of CommunityDragon `hashes.*.txt` files. Tables are memory-mapped and
  shared across LeagueToolkit tools, so there is no per-run parse of a ~250 MB text blob.
- `download-hashes` / `dl` now installs the published mimir tables into the shared cache
  (checksum-verified, atomic, incremental) instead of downloading CommunityDragon text files.
- `hashtable-dir` / `hd` now prints the mimir cache directory; `--hashtable-dir` and the
  `hashtable_dir` config value now override that cache directory (also settable via `MIMIR_DIR`).
- `-H/--hashtable <PATH>` still loads a supplemental `<hex-hash> <path>` text file, layered on
  top of the cache.

### Removed

- On first run after upgrading, the old `Documents/LeagueToolkit/wad_hashtables`
  `hashes.game.txt` / `hashes.lcu.txt` files are removed (best-effort; custom files are left
  untouched and the folder is deleted only if it becomes empty).

## [0.5.6](https://github.com/LeagueToolkit/wadtools/compare/v0.5.5...v0.5.6) - 2026-02-14

### Added

- better diff command

### Fixed

- update list_filters argument alias to use short form

### Other

- update league-toolkit to version 0.2.17

## [0.5.5](https://github.com/LeagueToolkit/wadtools/compare/v0.5.4...v0.5.5) - 2026-02-01

### Added

- collect extract stats
- hash filter
- incremental extraction
- parallel extraction with rayon

## [0.5.4](https://github.com/LeagueToolkit/wadtools/compare/v0.5.3...v0.5.4) - 2026-01-31

### Added

- add filter inversion support to extract and list commands
- support multiple input WAD files in extract and list commands
- add hashtable download command

### Fixed

- sort diffs by path_hash

### Other

- update league-toolkit to 0.2.15

## [0.5.3](https://github.com/LeagueToolkit/wadtools/releases/tag/v0.5.3) - 2025-11-25

### Added

- add list command
- better filter support
- save config next to executable and add hashtable dir override arg
- add command to show default hashtable directory in wadtools
- camino
- default hashtable dir
- config
- add verbosity level control for tracing output
- truncate long filenames in log and remove useless directory prep
- truncate long file names
- use ltk chunk extensions
- make regex case insensitive by default
- use fancy regex
- add aliases for extract and diff commands
- allow multiple filter types
- add extraction progress bar
- test
- workflows and sorting
- add diff command
- add extract command

### Fixed

- assign extension to hashed files
- formatting
- tracing output layers
- show correct number of extracted chunks

### Other

- update wadtools to version 0.5.3
- extract create_filter_pattern to shared utils module
- release v0.5.2
- move changelog to crate folder
- bump to v0.5.2
- bump wadtools version to 0.5.1
- update version to 0.5.0 and revise CHANGELOG for new release
- update README with config and hashtable discovery details; refactor diff and extract commands to load default hashtable directory
- remove redundant logging in add_from_dir method
- *(release)* v0.4.0
- satisfy format lints
- makes sure that hashed filenames are zero padded
- filter extract
- get rid of unstable features

## [0.5.2](https://github.com/LeagueToolkit/wadtools/releases/tag/v0.5.2) - 2025-11-23

### Added

- better filter support
- save config next to executable and add hashtable dir override arg
- add command to show default hashtable directory in wadtools
- camino
- default hashtable dir
- config
- add verbosity level control for tracing output
- truncate long filenames in log and remove useless directory prep
- truncate long file names
- use ltk chunk extensions
- make regex case insensitive by default
- use fancy regex
- add aliases for extract and diff commands
- allow multiple filter types
- add extraction progress bar
- test
- workflows and sorting
- add diff command
- add extract command

### Fixed

- assign extension to hashed files
- formatting
- tracing output layers
- show correct number of extracted chunks

### Other

- move changelog to crate folder
- bump to v0.5.2
- bump wadtools version to 0.5.1
- update version to 0.5.0 and revise CHANGELOG for new release
- update README with config and hashtable discovery details; refactor diff and extract commands to load default hashtable directory
- remove redundant logging in add_from_dir method
- *(release)* v0.4.0
- satisfy format lints
- makes sure that hashed filenames are zero padded
- filter extract
- get rid of unstable features

## [0.5.1](https://github.com/LeagueToolkit/wadtools/releases/tag/v0.5.1) - 2025-10-23

### Added

- add command to show default hashtable directory in wadtools
- camino
- default hashtable dir
- config
- add verbosity level control for tracing output
- truncate long filenames in log and remove useless directory prep
- truncate long file names
- use ltk chunk extensions
- make regex case insensitive by default
- use fancy regex
- add aliases for extract and diff commands
- allow multiple filter types
- add extraction progress bar
- test
- workflows and sorting
- add diff command
- add extract command

### Fixed

- formatting
- tracing output layers
- show correct number of extracted chunks

### Other

- bump wadtools version to 0.5.1
- update version to 0.5.0 and revise CHANGELOG for new release
- update README with config and hashtable discovery details; refactor diff and extract commands to load default hashtable directory
- remove redundant logging in add_from_dir method
- *(release)* v0.4.0
- satisfy format lints
- makes sure that hashed filenames are zero padded
- filter extract
- get rid of unstable features

## [0.4.0](https://github.com/LeagueToolkit/wadtools/releases/tag/v0.4.0) - 2025-10-08

- truncate long filenames in log and remove useless directory prep
- truncate long file names
- use ltk chunk extensions
- make regex case insensitive by default
- use fancy regex
- add aliases for extract and diff commands
- allow multiple filter types
- add extraction progress bar
- test
- workflows and sorting
- add diff command
- add extract command

### Fixed

- formatting
- tracing output layers
- show correct number of extracted chunks

### Other

- _(release)_ v0.4.0
- satisfy format lints
- makes sure that hashed filenames are zero padded
- filter extract
- get rid of unstable features
