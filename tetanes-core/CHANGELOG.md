<!-- markdownlint-disable-file no-duplicate-heading -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.10.1](https://github.com/lukexor/tetanes/compare/0.10.0..0.10.1) - 2024-05-22

### ⛰️  Features


- Added mapper 11 - ([03d2074](https://github.com/lukexor/tetanes/commit/03d2074d3d58fcf652fecb9d77f4e96e8c007aae))
- Updated game database mapper names - ([86d246b](https://github.com/lukexor/tetanes/commit/86d246be9a52b64ed4191c970c6a727a31c21cb5))

### 🐛 Bug Fixes


- Disable rewind when low on memory. clear rewind memory when disabled - ([4d5e1c4](https://github.com/lukexor/tetanes/commit/4d5e1c4dbe43cceb9ab8d4c33ca832830b2d31d8))

### 🚜 Refactor


- Add Sram trait and some mapper cleanup - ([ad03755](https://github.com/lukexor/tetanes/commit/ad0375506644f990e726c536f29bdf62d34d9e84))

### 📚 Documentation


- Fixed docs and changelog - ([4c7a694](https://github.com/lukexor/tetanes/commit/4c7a6949e52b6734fd6a78f6d9567c70e12b3ae4))
- Fixed docs - ([7a491c1](https://github.com/lukexor/tetanes/commit/7a491c14a2cb93db489c8bcb05d65f63bd1ed9d7))

### 🧪 Testing


- Avoid serde_json::from_reader in tests as it's faster to just … ([#244](https://github.com/lukexor/tetanes/pull/244)) - ([3ca03ac](https://github.com/lukexor/tetanes/commit/3ca03ac68fab4d809dee39466fd661f887d2575d))


## [0.10.0](https://github.com/lukexor/tetanes/compare/tetanes-v0.9.0..tetanes-core-v0.10.0) - 2024-05-16

Initial release.
