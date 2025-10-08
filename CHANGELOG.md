# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/LeagueToolkit/wadtools/releases/tag/v0.4.0) - 2025-10-08

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

- update README with config and hashtable discovery details; refactor diff and extract commands to load default hashtable directory
- remove redundant logging in add_from_dir method
- *(release)* v0.4.0
- satisfy format lints
- makes sure that hashed filenames are zero padded
- filter extract
- get rid of unstable features