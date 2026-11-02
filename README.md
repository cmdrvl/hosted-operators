# hosted-operators

Shared hosted runtime for selected CMD+RVL spine tools.

Current scope:

- `host-rvl`
- `host-shape`

Each hosted service exposes a consistent HTTP surface:

- `GET /health`
- `GET /describe`
- `GET /schema/output`
- `GET /version`
- `POST /v1/run`

## Development

Run `rvl` host:

```bash
cargo run -p host-rvl
```

Run `shape` host:

```bash
cargo run -p host-shape
```

Common environment variables:

- `CMDRVL_HOST_BIND` default `0.0.0.0`
- `CMDRVL_HOST_PORT` default `8080` and falls back to `PORT`
- `CMDRVL_HOST_API_TOKEN` optional bearer token
- `CMDRVL_HOST_ALLOWED_ORIGINS` comma-separated origins, or `*`
- `CMDRVL_HOST_MAX_BODY_MB` default `50`
- `CMDRVL_KOVREX_FILE_API` default `https://gateway.kovrex.ai/v1/files`

## Input model

Hosted file inputs use an explicit union:

```json
{
  "kind": "inline_base64",
  "content_b64": "..."
}
```

or

```json
{
  "kind": "kovrex_file",
  "file_id": "kvx_file_123"
}
```

`rvl` example:

```bash
curl -X POST http://localhost:8080/v1/run \
  -H "Authorization: Bearer $CMDRVL_HOST_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "old": { "kind": "inline_base64", "content_b64": "'"$(base64 -i old.csv)"'" },
    "new": { "kind": "inline_base64", "content_b64": "'"$(base64 -i new.csv)"'" },
    "key": "id",
    "threshold": 0.95,
    "tolerance": 1e-9
  }'
```

## Deployment

Build any hosted binary with the shared Dockerfile:

```bash
docker build --build-arg BIN_PACKAGE=host-rvl -t host-rvl .
docker build --build-arg BIN_PACKAGE=host-shape -t host-shape .
```

## Hosted domain retirement

As of 2026-11-02, `rvl.cmdrvl.com` and `shape.cmdrvl.com` are retired
deployment targets. Do not provision them or point new integrations at them.
The open-source `rvl` and `shape` tools and this repository's local HTTP
runtime remain available.

The authorized Route 53 zone inventory contains neither hostname. The
documented AWS account's `us-east-1` Lightsail inventory has no instances or
static IPs; CloudFront aliases and ACM certificate primary names contain no
matching domains. These bounded reads do not establish that every historical
deployment has been removed. Railway inventory requires a current login and
has not been verified. Full provider retirement remains tracked in
cmdrvl-website `bd-ubll.14`.

## Historical Railway deployment

This repo now includes service-specific Railway deployment files under [`deploy/railway/README.md`](deploy/railway/README.md).

Historical setup:

- Railway service `rvl` -> config file `/deploy/railway/rvl.json`
- Railway service `shape` -> config file `/deploy/railway/shape.json`

Those configs use service-specific Dockerfiles so each service builds the correct binary without relying on a dashboard-only build arg.

## Historical AWS deployment

This repo also includes a cheap single-host AWS deployment path under [`deploy/aws/README.md`](deploy/aws/README.md).

The retired setup used:

- one Lightsail instance
- one static IP
- `rvl.cmdrvl.com`
- `shape.cmdrvl.com`

The retained deployment files describe the former topology for investigation
and recovery. They do not establish a currently available hosted offer.
