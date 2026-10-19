# Changelog

All notable changes to Elocutio are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims
to follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Product audit, build plan, architecture, feature matrix, decision records,
  prompt library, and voice-learning design.
- Apache-2.0 license, NOTICE with Harper attribution, and open-source
  governance files (contributing, code of conduct, security policy).
- Rust workspace scaffold: `elocutio-core`, `elocutio-analysis`,
  `elocutio-llm`, `elocutio-voice`, `elocutio-store`, `elocutio-bridge`.
- Exact span resolver with UTF-16/grapheme offset tests.
- CI workflow, issue templates, pull request template, and Dependabot.
- Bootstrap, documentation-check, development-signing, and upstream-fetch
  scripts.
- Initial repository scaffold and pinned Harper v2.13.0 upstream lock.

### Changed

- Product name set to **Elocutio** (ADR-020).
- Build strategy set to fork Harper Desktop (ADR-001).
- License set to Apache-2.0 (ADR-002).

### Security

- Documented the key-handling, voice-sample, and local-bridge threat models.

## [0.0.0] - 2026-10-19

- Planning baseline.
