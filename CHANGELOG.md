# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.1](https://github.com/Dekker1/deepclone/compare/deepclone-v0.3.0...deepclone-v0.3.1) - 2026-08-28

### Added

- add a type-level `#[deepclone(clone)]`

### Other

- better reflect when `clone` might be preferred.

## [0.3.0](https://github.com/Dekker1/deepclone/compare/deepclone-v0.2.0...deepclone-v0.3.0) - 2026-08-27

### Added

- [**breaking**] replace `deep_clone_box` with unsized pointer helpers

## [0.2.0](https://github.com/Dekker1/deepclone/compare/deepclone-v0.1.0...deepclone-v0.2.0) - 2026-08-27

### Added

- [**breaking**] cover `Box<dyn Trait>` with a blanket impl
