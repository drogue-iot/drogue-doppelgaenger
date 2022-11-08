#!/usr/bin/env bash

source "${BASEDIR}/cmd/__endpoints.sh"

if [[ "$1" == "--fish" ]]; then
  progress set API_URL "$(get_cm_entry drogue-doppelgaenger-endpoints api-url)"
  progress set SSO_URL "$(get_cm_entry drogue-doppelgaenger-endpoints sso-url)"
  progress set DEBUGGER_URL "$(get_cm_entry drogue-doppelgaenger-endpoints debugger-url)"
else
  progress API_URL="$(get_cm_entry drogue-doppelgaenger-endpoints api-url)"
  progress SSO_URL="$(get_cm_entry drogue-doppelgaenger-endpoints sso-url)"
  progress DEBUGGER_URL="$(get_cm_entry drogue-doppelgaenger-endpoints debugger-url)"
fi