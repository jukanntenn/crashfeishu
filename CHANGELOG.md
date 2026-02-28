# Changelog

## [0.3.0] - 2026-02-28

### Added

- Batch mode to aggregate crash notifications and avoid message spam when processes crash repeatedly

## [0.2.0] - 2025-12-12

### Added

- Support for webhook URL via `CRASHFEISHU_WEBHOOK` environment variable
- Command-line parameter takes priority over environment variable

## [0.1.2] - 2025-09-21

### Added

- Multi-architecture support (x86_64, aarch64) via cross-compilation

## [0.1.0] - 2025-01-02

### Added

- Initial release
- Supervisor event listener for crash notifications
- Feishu webhook integration
- Process monitoring with optional process name filtering
- Support for process groups (group_name:process_name format)
