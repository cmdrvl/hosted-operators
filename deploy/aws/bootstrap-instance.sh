#!/usr/bin/env bash
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

apt-get update
apt-get install -y ca-certificates curl gnupg debian-keyring debian-archive-keyring apt-transport-https unzip
apt-get install -y caddy

if ! id -u cmdrvl >/dev/null 2>&1; then
	useradd --system --create-home --home-dir /var/lib/cmdrvl --shell /usr/sbin/nologin cmdrvl
fi

install -d -o cmdrvl -g cmdrvl /opt/cmdrvl
install -d -o cmdrvl -g cmdrvl /opt/cmdrvl/bin
install -d -o cmdrvl -g cmdrvl /var/lib/cmdrvl
install -d -o root -g root /etc/cmdrvl

systemctl enable caddy
