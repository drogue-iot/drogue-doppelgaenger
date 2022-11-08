#!/usr/bin/env bash

set -ex

BASEDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"

eval "$("${BASEDIR}/../../scripts/drgadm" endpoints)"

# get OAuth token

SECRET=$(kubectl -n drogue-doppelgaenger get secret keycloak-client-secret-services -o json | jq -r .data.CLIENT_SECRET | base64 -d)
TOKEN=$(http --form POST "$SSO_URL/realms/doppelgaenger/protocol/openid-connect/token" grant_type=client_credentials client_id=services "client_secret=$SECRET")
echo "Token: $TOKEN"

HTTP_OPTS="-A bearer -a $(echo "$TOKEN" | jq -r .access_token)"

# do some basic stuff

# shellcheck disable=SC2086
http $HTTP_OPTS POST "$API_URL/api/v1alpha1/things" metadata:='{"name": "foo", "application": "default"}'
# shellcheck disable=SC2086
http $HTTP_OPTS PUT "$API_URL/api/v1alpha1/things/default/things/foo/reportedStates" temperature:=42
# shellcheck disable=SC2086
http $HTTP_OPTS GET "$API_URL/api/v1alpha1/things/default/things/foo"
# shellcheck disable=SC2086
http $HTTP_OPTS DELETE "$API_URL/api/v1alpha1/things/default/things/foo"
