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

## Channel9 Device Login Contract

Channel9 device-code login follows the SaaS spec in `docs/channel9-device-code-login.md`.

- Firmware creates a device code with `device_id` and `interfaces` only.
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
