# AWS

Cheap direct hosting for the public CMD+RVL operators.

This deployment path keeps Kovrex as the marketplace surface and replaces Railway with a small AWS origin under `cmdrvl.com`.

## Topology

- one Lightsail instance in `us-east-1`
- one public static IP
- `rvl.cmdrvl.com` -> same host
- `shape.cmdrvl.com` -> same host
- Caddy terminates HTTPS and routes by hostname
- `host-rvl` listens on `127.0.0.1:8081`
- `host-shape` listens on `127.0.0.1:8082`

This avoids API Gateway payload limits and keeps monthly cost low.

## Defaults

- instance name: `hosted-operators-1`
- static IP name: `hosted-operators-ip`
- region: `us-east-1`
- availability zone: `us-east-1a`
- blueprint: `ubuntu_24_04`
- bundle: `nano_2_0`
- key pair name: `hosted-operators-main`

## Prerequisites

- AWS CLI authenticated with `AWS_PROFILE=foodtruck_profile`
- local SSH public key at `~/.ssh/id_ed25519.pub`
- local Docker
- local `ssh` and `scp`
- Route 53 hosted zone for `cmdrvl.com`

## Provision the host

Create the Lightsail instance, static IP, firewall rules, and DNS records:

```bash
AWS_PROFILE=foodtruck_profile \
AWS_REGION=us-east-1 \
./deploy/aws/provision.sh
```

That script creates:

- Lightsail instance `hosted-operators-1`
- static IP `hosted-operators-ip`
- `A` record `rvl.cmdrvl.com`
- `A` record `shape.cmdrvl.com`

## Bootstrap the instance

SSH in as `ubuntu` and run:

```bash
sudo bash -s < deploy/aws/bootstrap-instance.sh
```

That installs Caddy, creates the `cmdrvl` service user, and creates the runtime directories.

## Deploy

Set the hosted API token in your shell and deploy from this repo:

```bash
export CMDRVL_HOST_API_TOKEN='replace-me'
export CMDRVL_HOST_ALLOWED_ORIGINS='https://www.kovrex.ai,https://kovrex.ai'

./deploy/aws/deploy.sh "$(dig +short rvl.cmdrvl.com | tail -n 1)"
```

The deploy script:

- builds Linux binaries locally with Docker
- uploads `host-rvl` and `host-shape`
- installs systemd units and the Caddy config
- writes `/etc/cmdrvl/host-rvl.env`
- writes `/etc/cmdrvl/host-shape.env`
- restarts `caddy`, `host-rvl`, and `host-shape`

## Verify

```bash
curl https://rvl.cmdrvl.com/health
curl https://shape.cmdrvl.com/health
curl https://rvl.cmdrvl.com/version
curl https://shape.cmdrvl.com/version
```

## Cut over Kovrex

Point the existing operator integrations at:

- `https://rvl.cmdrvl.com`
- `https://shape.cmdrvl.com`

## Notes

- `nano_2_0` is the cheapest Lightsail bundle with public IPv4 included.
- If runtime memory is tight, resize the instance to `micro_2_0`.
- This path keeps build work on the local machine, not on the instance.
