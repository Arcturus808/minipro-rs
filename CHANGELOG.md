# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2025-01-01

### Added
- Initial Rust reimplementation of [minipro](https://gitlab.com/DavidGriffith/minipro)
- Pure-Rust USB layer via `nusb` — no libusb, no Cygwin, single static binary on all platforms
- Support for XGecu TL866A, TL866CS, TL866II+, T48, T56, and T76 programmers
- Read, write, verify, erase, and blank-check operations for EEPROM/Flash/PIC/AVR devices
- File format support: Intel HEX (`.hex`), Motorola SREC (`.srec`/`.mot`), JEP-106 JEDEC (`.jed`), raw binary (`.bin`)
- `--format` flag for explicit format selection (default: auto-detect from extension)
- Fuse/configuration-bit operations: `--read-fuses`, `--write-fuses`
- SPI flash JEDEC ID autodetection: `--spi-autodetect`
- Logic IC test via external `logicic.xml` database: `--logic-test`
- Firmware update support for TL866II+, T48, and T76: `--firmware-update`
- ZIF socket pin-level control (voltages, pull-downs, OVC check)
- ICSP (in-circuit serial programming) mode: `--icsp`
- Shell completion generation for Bash, Zsh, Fish, and PowerShell: `--generate-completions`
- Windows MSI installer via `cargo-wix`
- Linux `.deb` and `.rpm` packages via `cargo-deb` / `cargo-generate-rpm`
- GitLab CI/CD pipeline with check, test, build, package, and release stages
- Integration test framework with `MockUsb` fixture replay (no hardware required)
- `cargo doc` API documentation for all public types
