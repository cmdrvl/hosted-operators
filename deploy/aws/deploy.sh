#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
	echo "usage: $0 <host-or-ip>" >&2
	exit 1
fi

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
BUILD_DIR="$ROOT_DIR/deploy/aws/.build"
HOST="$1"
SSH_USER="${CMDRVL_HOST_SSH_USER:-ubuntu}"
REMOTE="${SSH_USER}@${HOST}"
API_TOKEN="${CMDRVL_HOST_API_TOKEN:-}"
ALLOWED_ORIGINS="${CMDRVL_HOST_ALLOWED_ORIGINS:-*}"
KOVREX_FILE_API="${CMDRVL_KOVREX_FILE_API:-https://gateway.kovrex.ai/v1/files}"
RUST_LOG_LEVEL="${RUST_LOG:-info}"

if [[ -z "$API_TOKEN" ]]; then
	echo "CMDRVL_HOST_API_TOKEN is required" >&2
	exit 1
fi

mkdir -p "$BUILD_DIR"

build_binary() {
	local package="$1"
	local image_tag="hosted-operators-${package}:local"
	local container_name
	local target_path="$BUILD_DIR/${package}.bin"
	container_name="$(echo "$image_tag" | tr ':/' '__')"

	docker build --build-arg BIN_PACKAGE="$package" -t "$image_tag" "$ROOT_DIR" >/dev/null
	docker rm -f "$container_name" >/dev/null 2>&1 || true
	docker create --name "$container_name" "$image_tag" >/dev/null
	docker cp "$container_name:/usr/local/bin/app" "$target_path"
	docker rm -f "$container_name" >/dev/null
	chmod +x "$target_path"
}

write_env_file() {
	local port="$1"
	local target="$2"
	cat > "$target" <<EOF
CMDRVL_HOST_BIND=127.0.0.1
CMDRVL_HOST_PORT=${port}
CMDRVL_HOST_API_TOKEN=${API_TOKEN}
CMDRVL_HOST_ALLOWED_ORIGINS=${ALLOWED_ORIGINS}
CMDRVL_KOVREX_FILE_API=${KOVREX_FILE_API}
RUST_LOG=${RUST_LOG_LEVEL}
EOF
}

build_binary "host-rvl"
build_binary "host-shape"
write_env_file 8081 "$BUILD_DIR/host-rvl.env"
write_env_file 8082 "$BUILD_DIR/host-shape.env"

scp \
	"$BUILD_DIR/host-rvl.bin" \
	"$BUILD_DIR/host-shape.bin" \
	"$BUILD_DIR/host-rvl.env" \
	"$BUILD_DIR/host-shape.env" \
	"$ROOT_DIR/deploy/aws/Caddyfile" \
	"$ROOT_DIR/deploy/aws/systemd/host-rvl.service" \
	"$ROOT_DIR/deploy/aws/systemd/host-shape.service" \
	"$REMOTE:/tmp/"

ssh "$REMOTE" '
set -euo pipefail
sudo install -d -o cmdrvl -g cmdrvl /opt/cmdrvl/bin
sudo install -d -o root -g root /etc/cmdrvl
sudo install -o cmdrvl -g cmdrvl -m 0755 /tmp/host-rvl.bin /opt/cmdrvl/bin/host-rvl
sudo install -o cmdrvl -g cmdrvl -m 0755 /tmp/host-shape.bin /opt/cmdrvl/bin/host-shape
sudo install -o root -g root -m 0644 /tmp/host-rvl.env /etc/cmdrvl/host-rvl.env
sudo install -o root -g root -m 0644 /tmp/host-shape.env /etc/cmdrvl/host-shape.env
sudo install -o root -g root -m 0644 /tmp/host-rvl.service /etc/systemd/system/host-rvl.service
sudo install -o root -g root -m 0644 /tmp/host-shape.service /etc/systemd/system/host-shape.service
sudo install -o root -g root -m 0644 /tmp/Caddyfile /etc/caddy/Caddyfile
sudo systemctl daemon-reload
sudo systemctl enable --now host-rvl host-shape caddy
sudo systemctl restart host-rvl host-shape caddy
sudo systemctl --no-pager --full status host-rvl host-shape caddy | sed -n "1,160p"
rm -f /tmp/host-rvl.bin /tmp/host-shape.bin /tmp/host-rvl.env /tmp/host-shape.env /tmp/host-rvl.service /tmp/host-shape.service /tmp/Caddyfile
'
