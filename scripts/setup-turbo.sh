#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly ROOT_PACKAGE_FILE="${REPOSITORY_ROOT}/package.json"
readonly APP_PACKAGE_FILE="${REPOSITORY_ROOT}/apps/my_supervisor/package.json"
readonly DESKTOP_PACKAGE_FILE="${REPOSITORY_ROOT}/apps/my_supervisor/desktop/package.json"
readonly PNPM_WORKSPACE_FILE="${REPOSITORY_ROOT}/pnpm-workspace.yaml"
readonly TURBO_FILE="${REPOSITORY_ROOT}/turbo.json"

ensure_pnpm_is_installed() {
  if command -v pnpm >/dev/null 2>&1; then
    return
  fi

  echo "pnpm 명령을 찾을 수 없습니다. 먼저 ./scripts/setup-proto.sh를 실행하세요." >&2
  exit 1
}

ensure_workspace_directories() {
  mkdir -p \
    "${REPOSITORY_ROOT}/apps/my_supervisor" \
    "${REPOSITORY_ROOT}/apps/my_supervisor/desktop"
}

ensure_root_package_file() {
  if [[ -f "${ROOT_PACKAGE_FILE}" ]]; then
    echo "package.json 파일을 재사용합니다."
    return
  fi

  cat <<'EOF' > "${ROOT_PACKAGE_FILE}"
{
  "name": "my-supervisor",
  "version": "0.1.0",
  "private": true,
  "packageManager": "pnpm@10.11.0",
  "scripts": {
    "dev": "turbo run dev --filter=@my-supervisor/desktop",
    "build": "turbo run build",
    "build:rust": "turbo run build --filter=@my-supervisor/my-supervisor",
    "build:desktop": "turbo run build --filter=@my-supervisor/desktop",
    "typecheck": "turbo run typecheck",
    "typecheck:rust": "turbo run typecheck --filter=@my-supervisor/my-supervisor",
    "typecheck:desktop": "turbo run typecheck --filter=@my-supervisor/desktop",
    "desktop:dev": "pnpm --filter @my-supervisor/desktop desktop:dev",
    "desktop:build": "pnpm --filter @my-supervisor/desktop desktop:build"
  },
  "devDependencies": {
    "turbo": "^2.10.3"
  }
}
EOF

  echo "package.json 파일을 생성했습니다."
}

ensure_app_package_file() {
  if [[ -f "${APP_PACKAGE_FILE}" ]]; then
    echo "apps/my_supervisor/package.json 파일을 재사용합니다."
    return
  fi

  cat <<'EOF' > "${APP_PACKAGE_FILE}"
{
  "name": "@my-supervisor/my-supervisor",
  "version": "0.1.0",
  "private": true,
  "scripts": {
    "build": "cargo build --workspace",
    "typecheck": "cargo check --workspace"
  }
}
EOF

  echo "apps/my_supervisor/package.json 파일을 생성했습니다."
}

ensure_desktop_package_file() {
  if [[ -f "${DESKTOP_PACKAGE_FILE}" ]]; then
    echo "apps/my_supervisor/desktop/package.json 파일을 재사용합니다."
    return
  fi

  cat <<'EOF' > "${DESKTOP_PACKAGE_FILE}"
{
  "name": "@my-supervisor/desktop",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "desktop:dev": "cargo tauri dev",
    "desktop:build": "cargo tauri build",
    "preview": "vite preview",
    "typecheck": "tsc -b --pretty false"
  }
}
EOF

  echo "apps/my_supervisor/desktop/package.json 파일을 생성했습니다."
}

ensure_pnpm_workspace_file() {
  if [[ -f "${PNPM_WORKSPACE_FILE}" ]]; then
    echo "pnpm-workspace.yaml 파일을 재사용합니다."
    return
  fi

  cat <<'EOF' > "${PNPM_WORKSPACE_FILE}"
packages:
  - "apps/*"
  - "apps/*/desktop"
EOF

  echo "pnpm-workspace.yaml 파일을 생성했습니다."
}

ensure_turbo_file() {
  if [[ -f "${TURBO_FILE}" ]]; then
    echo "turbo.json 파일을 재사용합니다."
    return
  fi

  cat <<'EOF' > "${TURBO_FILE}"
{
  "$schema": "https://turborepo.com/schema.json",
  "tasks": {
    "build": {
      "dependsOn": ["^build"],
      "outputs": ["dist/**", "target/**"]
    },
    "typecheck": {
      "dependsOn": ["^typecheck"],
      "outputs": []
    },
    "dev": {
      "cache": false,
      "persistent": true
    }
  }
}
EOF

  echo "turbo.json 파일을 생성했습니다."
}

print_next_step() {
  echo
  echo "의존성 설치가 필요하면 아래 명령을 실행하세요:"
  echo "  pnpm install"
}

main() {
  ensure_pnpm_is_installed
  ensure_workspace_directories
  ensure_root_package_file
  ensure_app_package_file
  ensure_desktop_package_file
  ensure_pnpm_workspace_file
  ensure_turbo_file
  print_next_step
}

main "$@"
