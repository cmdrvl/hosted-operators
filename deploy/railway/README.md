# Railway

This directory contains the service-specific Railway configuration for the first hosted operators:

- `rvl`
- `shape`

## Why service-specific files?

Railway supports config as code and custom Dockerfile paths, but each service still needs its own deployment settings.

This repo is a shared Rust workspace, so the cleanest Railway setup is:

- one GitHub repo
- one Railway service for `rvl`
- one Railway service for `shape`
- one service-specific config file per deployment
- one service-specific Dockerfile per deployment

That keeps the deploy surface explicit and avoids relying on a mutable dashboard-only `BIN_PACKAGE` variable.

## Service setup

Create two services in the same Railway project from this GitHub repo:

1. `rvl`
2. `shape`

For both services:

- Source repo: `cmdrvl/hosted-operators`
- Root directory: `/`

Then set the Railway config file path per service:

- `rvl`: `/deploy/railway/rvl.json`
- `shape`: `/deploy/railway/shape.json`

## Required environment variables

Set these in each Railway service:

- `CMDRVL_HOST_API_TOKEN`
- `CMDRVL_HOST_ALLOWED_ORIGINS`

Optional:

- `CMDRVL_KOVREX_FILE_API`
- `CMDRVL_HOST_MAX_BODY_MB`
- `RUST_LOG`

Do not set `CMDRVL_HOST_PORT`; Railway injects `PORT`, and the runtime already falls back to it.

## Cutover order

1. Deploy `shape` from this repo.
2. Deploy `rvl` from this repo.
3. Verify `/health`, `/describe`, `/schema/output`, and `/v1/run` on the new `rvl` service.
4. Move the existing Kovrex/operator URL or Railway domain to the new `rvl` service.
5. Retire `rvl-kovrex` once traffic is confirmed on the shared repo deployment.
