# Channel9 Device Code Login

This document defines the firmware-side contract for Channel9 device-code login.
It mirrors the LinkerDog SaaS Channel9 device-code flow.

## Actors

- Device: ESP32-S3 firmware running on Cardputer-Adv or another Channel9 board.
- Browser: a signed-in LinkerDog user session.
- Config API: the Channel9 control-plane API.

## Create Device Code

```http
POST /api/v1/channel9/device-codes
Content-Type: application/json
```

Request:

```json
{
  "device_id": "cardputer-adv",
  "interfaces": ["display", "speaker"]
}
```

Important rules:

- The request does not include `workspace_id`.
- The pending device code is not workspace-bound.
- The firmware displays `user_code` after the response succeeds.
- The human-visible `user_code` uses `-` as its separator, for example `ABCD-1234`.
- The firmware may also store `device_code` in RAM while the login flow is active.

Response:

```json
{
  "device_code": "ch9dc_...",
  "user_code": "ABCD-1234",
  "verification_uri": "https://app.linkerdog.work/channel9/device",
  "verification_uri_complete": "https://app.linkerdog.work/channel9/device?user_code=ABCD-1234",
  "expires_at": 1790000000,
  "activation_expires_at": 1787408600,
  "interval_seconds": 5
}
```

## Approve Device Code

Approval happens in the browser:

```http
POST /api/v1/channel9/device-codes/approve
Cookie: ld_account_session=...
Content-Type: application/json
```

Request:

```json
{
  "user_code": "ABCD-1234"
}
```

The server resolves the signed-in account's default workspace during approval and binds the
pending device code to that workspace. The device does not know or choose the workspace before this
step.

## Poll Device Token

```http
POST /api/v1/channel9/device-codes/token
Content-Type: application/json
```

Request:

```json
{
  "device_code": "ch9dc_..."
}
```

Before browser approval, the server returns `authorization_pending`. After approval, the server
returns a device token:

```json
{
  "access_token": "...",
  "token_type": "Bearer",
  "expires_at": 1790000000,
  "workspace_id": "ws_...",
  "device_id": "cardputer-adv"
}
```

The firmware persists `access_token`, `token_expires_at`, `workspace_id`, and `device_id` after an
approved response.

## Firmware UI Behavior

- If no token is stored, the Channel9 settings screen shows login state and a `Refresh` action.
- Opening the Channel9 settings screen must not implicitly start an HTTPS request.
- Selecting `Refresh` creates a new device code.
- Selecting `Poll` checks whether the browser has approved the active device code.
- If a token is stored, the settings screen displays login status and metadata, including
  `workspace_id`.
- Clearing login removes the token metadata and returns to the device-code login flow.

## Error Semantics

- `authorization_pending`: the browser has not approved the code yet.
- `channel9_device_code_expired`: the activation window expired.
- `channel9_device_code_not_found`: the code was swept, cleaned up, or never existed.
- Network failures should be shown as a short actionable status on the device screen and logged over
  serial with the detailed error.
