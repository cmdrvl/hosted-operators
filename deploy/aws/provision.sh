#!/usr/bin/env bash
set -euo pipefail

AWS_PROFILE="${AWS_PROFILE:-foodtruck_profile}"
AWS_REGION="${AWS_REGION:-us-east-1}"
INSTANCE_NAME="${INSTANCE_NAME:-hosted-operators-1}"
STATIC_IP_NAME="${STATIC_IP_NAME:-hosted-operators-ip}"
KEY_PAIR_NAME="${KEY_PAIR_NAME:-hosted-operators-main}"
HOSTED_ZONE_ID="${HOSTED_ZONE_ID:-Z0956876K6M390PUC5IT}"
PUBLIC_KEY_PATH="${PUBLIC_KEY_PATH:-$HOME/.ssh/id_ed25519.pub}"
BLUEPRINT_ID="${BLUEPRINT_ID:-ubuntu_24_04}"
BUNDLE_ID="${BUNDLE_ID:-nano_2_0}"
AVAILABILITY_ZONE="${AVAILABILITY_ZONE:-us-east-1a}"
RVL_RECORD="${RVL_RECORD:-rvl.cmdrvl.com}"
SHAPE_RECORD="${SHAPE_RECORD:-shape.cmdrvl.com}"

aws_cmd() {
	AWS_PROFILE="$AWS_PROFILE" AWS_REGION="$AWS_REGION" AWS_DEFAULT_REGION="$AWS_REGION" aws "$@"
}

if [[ ! -f "$PUBLIC_KEY_PATH" ]]; then
	echo "missing SSH public key: $PUBLIC_KEY_PATH" >&2
	exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
	echo "jq is required" >&2
	exit 1
fi

if ! aws_cmd lightsail get-key-pairs --output json | jq -e --arg name "$KEY_PAIR_NAME" '.keyPairs[]? | select(.name == $name)' >/dev/null; then
	aws_cmd lightsail import-key-pair \
		--key-pair-name "$KEY_PAIR_NAME" \
		--public-key-base64 "$(cat "$PUBLIC_KEY_PATH")"
fi

if ! aws_cmd lightsail get-instance --instance-name "$INSTANCE_NAME" >/dev/null 2>&1; then
	aws_cmd lightsail create-instances \
		--instance-names "$INSTANCE_NAME" \
		--availability-zone "$AVAILABILITY_ZONE" \
		--blueprint-id "$BLUEPRINT_ID" \
		--bundle-id "$BUNDLE_ID" \
		--key-pair-name "$KEY_PAIR_NAME"
fi

for _ in $(seq 1 60); do
	state="$(aws_cmd lightsail get-instance-state --instance-name "$INSTANCE_NAME" --query 'state.name' --output text)"
	if [[ "$state" == "running" ]]; then
		break
	fi
	sleep 5
done

for port in 22 80 443; do
	if ! aws_cmd lightsail get-instance-port-states --instance-name "$INSTANCE_NAME" --output json \
		| jq -e --argjson port "$port" '.portStates[]? | select(.fromPort == $port and .toPort == $port and .protocol == "tcp" and .state == "open")' >/dev/null
	then
		aws_cmd lightsail open-instance-public-ports \
			--instance-name "$INSTANCE_NAME" \
			--port-info fromPort="$port",toPort="$port",protocol=TCP
	fi
done

if ! aws_cmd lightsail get-static-ip --static-ip-name "$STATIC_IP_NAME" >/dev/null 2>&1; then
	aws_cmd lightsail allocate-static-ip --static-ip-name "$STATIC_IP_NAME"
fi

attached_to="$(aws_cmd lightsail get-static-ip --static-ip-name "$STATIC_IP_NAME" --query 'staticIp.attachedTo' --output text)"
if [[ "$attached_to" != "$INSTANCE_NAME" ]]; then
	aws_cmd lightsail attach-static-ip --static-ip-name "$STATIC_IP_NAME" --instance-name "$INSTANCE_NAME"
fi

public_ip="$(aws_cmd lightsail get-static-ip --static-ip-name "$STATIC_IP_NAME" --query 'staticIp.ipAddress' --output text)"

tmpfile="$(mktemp)"
cat > "$tmpfile" <<EOF
{
  "Changes": [
    {
      "Action": "UPSERT",
      "ResourceRecordSet": {
        "Name": "${RVL_RECORD}",
        "Type": "A",
        "TTL": 300,
        "ResourceRecords": [{ "Value": "${public_ip}" }]
      }
    },
    {
      "Action": "UPSERT",
      "ResourceRecordSet": {
        "Name": "${SHAPE_RECORD}",
        "Type": "A",
        "TTL": 300,
        "ResourceRecords": [{ "Value": "${public_ip}" }]
      }
    }
  ]
}
EOF

aws_cmd route53 change-resource-record-sets \
	--hosted-zone-id "$HOSTED_ZONE_ID" \
	--change-batch "file://$tmpfile"

rm -f "$tmpfile"

echo "instance: $INSTANCE_NAME"
echo "ip: $public_ip"
echo "ssh: ssh ubuntu@$public_ip"
