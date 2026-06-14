# Agent Instructions

## Project

This repository contains Rust firmware for Channel9 on ESP32-S3 class devices.
The current board implementation is Cardputer-Adv.

## Coding Rules

- Keep code, comments, commit messages, and documentation in English.
- Prefer small, reviewable changes that follow the existing crate boundaries.
- Use `rg` for code search.
- Use `cargo fmt --check` and `cargo check` before reporting firmware changes as ready.
- Do not remove or rewrite unrelated user changes.

## Testing

- Use `cargo fmt --check` and `cargo check` for firmware-level validation.
- Use `cargo +<host-toolchain> test -p channel9-runtime --target <host-triple>` for host-side
  Channel9 runtime state-machine tests. The explicit toolchain is required because this repository's
  `rust-toolchain.toml` points at the ESP `esp` toolchain, and the host toolchain needs `rust-src`
  because `.cargo/config.toml` enables `build-std`. Get the local host triple from
  `rustc -vV`; GitHub Actions uses `cargo +nightly test -p channel9-runtime --target
  x86_64-unknown-linux-gnu`.
- Keep host-testable Channel9 state transitions in `crates/channel9-runtime`; avoid adding ESP-IDF,
  board, WiFi, display, or storage dependencies to that crate.
- Add or update `channel9-runtime` tests when changing device-code login, token polling, SSE polling,
  push rendering state, network queue backoff, stale response handling, or logout/offline cleanup.
- Keep GitHub Actions split by purpose: `.github/workflows/ci.yml` validates firmware with the ESP
  toolchain, while `.github/workflows/host-tests.yml` runs host-side Rust tests.

## Channel9 Device Login Contract

Channel9 device-code login follows the SaaS spec in `docs/channel9-device-code-login.md`.

- Firmware creates a device code with `device_id`, `interfaces`, and optional `description`.
- Firmware may include `description` in the device-code create request. Treat it as non-privileged human metadata.
- Firmware may replace a pending, unapproved in-memory `device_code`.
- After creating a device code, firmware polls the token endpoint every returned `interval_seconds` until approved or replaced.
- Firmware must stop polling an unapproved code after its `activation_expires_at`.
- Once a device token is stored, firmware must not create another device code unless login is cleared.
- Firmware must not send `workspace_id` when creating a device code.
- A pending device code is not workspace-bound.
- Browser approval binds the pending code to the signed-in account's default workspace.
- Firmware receives `workspace_id` only after token polling returns an approved device token.
- `Channel9Config.workspace_id` stores `workspace_id` metadata from the token response, not a user input for login.
- `Channel9Config.api_base_url` stores the Channel9 API endpoint. It defaults to `https://app.linkerdog.work` and must not be sent in the device-code request body.
- The Channel9 settings screen should not ask the user to type `workspace_id` before showing a device code.
- The human-visible user code uses `-` as its separator, for example `ABCD-1234`.

## Runtime Caution

Avoid starting network requests implicitly when opening a settings screen. On Cardputer-Adv,
WiFi/BLE coexistence and HTTPS work can run on ESP-IDF system tasks, so UI navigation should stay
side-effect-light. Trigger Channel9 network work from explicit actions such as `Refresh` or `Poll`.
