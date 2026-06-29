# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-06-29

### Added
- **Hermes support**: watch OpenAI-compatible sessions from the Hermes SQLite database (`~/.hermes/state.db`), auto-detected or via `--hermes-db`.
- **Hot-detection of new Hermes sessions**: viztokens now watches `~/.hermes/state.db` for new sessions created after startup and attaches a watcher automatically, mirroring the existing Claude Code hot-detection.
- **Tiktoken token estimation**: messages without exact token counts (Hermes or Claude Code) now show an estimated count prefixed with `~` (e.g. `~out:42`), using the o200k_base encoding as fallback.

### Changed
- **Code reorganisation**: Claude Code support extracted into `src/claude/` (parser, session discovery, file watcher), mirroring the existing `src/hermes/` structure. `src/watcher/` now contains only shared types (`WatcherEvent`, `Watcher`, `DiscoveredSession`).
- Filter changes (`f`, `F`, `1`–`4`) now reset the view to follow mode, preventing a blank screen when the filtered content is shorter than the previous scroll offset.
- Session-level token totals (Hermes) are no longer shown as per-message counts on the last assistant message; they remain visible in the status bar only.

## [0.2.0] - 2026-06-25

### Added
- Hot-detection of new Claude Code sessions: viztokens now watches `~/.claude/projects/` for new `.jsonl` files and attaches a watcher automatically, without requiring a restart.

