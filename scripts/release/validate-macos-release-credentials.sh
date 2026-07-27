#!/usr/bin/env bash
set -euo pipefail

missing=()
[[ -n "${APPLE_SIGNING_IDENTITY:-}" ]] || missing+=(APPLE_SIGNING_IDENTITY)

has_api_credentials=false
if [[ -n "${APPLE_API_KEY:-}" || -n "${APPLE_API_KEY_PATH:-}" ]]; then
  if [[ -n "${APPLE_API_ISSUER:-}" && -n "${APPLE_API_KEY_ID:-}" ]]; then
    has_api_credentials=true
  else
    missing+=(APPLE_API_ISSUER APPLE_API_KEY_ID)
  fi
fi

has_apple_id_credentials=false
if [[ -n "${APPLE_ID:-}" || -n "${APPLE_PASSWORD:-}" || -n "${APPLE_TEAM_ID:-}" ]]; then
  for variable_name in APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
    [[ -n "${!variable_name:-}" ]] || missing+=("${variable_name}")
  done
  [[ "${#missing[@]}" == 0 ]] && has_apple_id_credentials=true
fi

if [[ "${has_api_credentials}" != true && "${has_apple_id_credentials}" != true ]]; then
  if [[ -z "${APPLE_API_KEY:-}" && -z "${APPLE_API_KEY_PATH:-}" && -z "${APPLE_ID:-}" && -z "${APPLE_PASSWORD:-}" && -z "${APPLE_TEAM_ID:-}" ]]; then
    missing+=(APPLE_API_KEY_OR_PATH APPLE_API_ISSUER APPLE_API_KEY_ID OR_APPLE_ID_PASSWORD_TEAM_ID)
  fi
fi

if [[ "${#missing[@]}" != 0 ]]; then
  printf 'macOS release credential preflight failed; missing variable names: %s\n' "${missing[*]}" >&2
  exit 2
fi

printf 'macOS release credential preflight passed; values were not printed\n'
