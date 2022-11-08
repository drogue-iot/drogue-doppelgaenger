#!/usr/bin/env bash

set -ex

BASEDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"

eval "$("${BASEDIR}/script/drgadm" endpoints)"

# get OAuth token

HTTP_OPTS="-A bearer -a $(http --form POST "$SSO_URL/realms/doppelgaenger/protocol/openid-connect/token" grant_type=client_credentials client_id=services client_secret=bfa33cd2-18bd-11ed-a9f9-d45d6455d2cc | jq -r .access_token)"

# do some basic stuff

# shellcheck disable=SC2086
http $HTTP_OPTS POST "$SSO_URL/api/v1alpha1/things" metadata:='{"name": "foo", "application": "default"}'
# shellcheck disable=SC2086
http $HTTP_OPTS PUT "$SSO_URL/api/v1alpha1/things/default/things/foo/reportedStates" temperature:=42
# shellcheck disable=SC2086
http $HTTP_OPTS GET "$SSO_URL/api/v1alpha1/things/default/things/foo"
# shellcheck disable=SC2086
http $HTTP_OPTS DELETE "$SSO_URL/api/v1alpha1/things/default/things/foo"
