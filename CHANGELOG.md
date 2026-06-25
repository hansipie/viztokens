# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-06-25

### Added
- Hot-detection of new Claude Code sessions: viztokens now watches `~/.claude/projects/` for new `.jsonl` files and attaches a watcher automatically, without requiring a restart.

