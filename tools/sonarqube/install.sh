#!/bin/sh
# ADR: docs/adr/ADR-0017-push-boundary-sonarqube-verification.md
# Install pinned verification tools and activate this checkout's versioned hooks.
set -eu
root=$(git rev-parse --show-toplevel)
previous=$(git config --local --get core.hooksPath || true)
case "$previous" in
  ""|.githooks) ;;
  *) echo "SONAR_EXISTING_HOOKS: integrate existing hooks before installation" >&2; exit 1 ;;
esac
if [ -z "$previous" ]; then
  for hook in pre-commit pre-push; do
    path=$(git rev-parse --git-path "hooks/$hook")
    if [ -f "$path" ]; then
      echo "SONAR_EXISTING_HOOKS: $hook already exists" >&2
      exit 1
    fi
  done
fi
chmod +x "$root/.githooks/pre-push"
git config --local core.hooksPath .githooks
echo "SonarQube pre-push hook enabled."
