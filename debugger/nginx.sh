#!/usr/bin/env bash

set -e
set -x
set -o pipefail

: "${API_URL:=http://localhost:8011}"

: "${KEYCLOAK_URL:=http://localhost:8012}"
: "${REALM:=doppelgaenger}"
: "${CLIENT_ID:=api}"

: "${BACKEND_JSON:="{}"}"
: "${BACKEND_JSON_FILE:=/etc/config/login/backend.json}"

echo "Setting backend information:"

if [ -f "$BACKEND_JSON_FILE" ]; then
    echo "Using base config from file: $BACKEND_JSON_FILE"
    BACKEND_JSON="$(cat "$BACKEND_JSON_FILE")"
fi

# inject backend URL
echo "$BACKEND_JSON" | jq --arg value "$API_URL" '. + {api: $value}' | tee /endpoints/backend.json

# inject oauth2 information
jq --arg value "$CLIENT_ID" '.keycloak += {clientId: $value}' < /endpoints/backend.json | tee /endpoints/backend.json.tmp
mv /endpoints/backend.json.tmp /endpoints/backend.json
jq --arg value "$KEYCLOAK_URL" '.keycloak += {url: $value}' < /endpoints/backend.json | tee /endpoints/backend.json.tmp
mv /endpoints/backend.json.tmp /endpoints/backend.json
jq --arg value "$REALM" '.keycloak += {realm: $value}' < /endpoints/backend.json | tee /endpoints/backend.json.tmp
mv /endpoints/backend.json.tmp /endpoints/backend.json

echo "Final backend information:"
echo "---"
cat /endpoints/backend.json
echo "---"

exec /usr/sbin/nginx -g "daemon off;"
