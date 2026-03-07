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
