#!/bin/sh
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
python3 -m venv "$root/tools/sonarqube/.venv"
"$root/tools/sonarqube/.venv/bin/python" -m pip install --disable-pip-version-check -r "$root/tools/sonarqube/requirements.txt"
npm ci --prefix "$root/tools/sonarqube"
chmod +x "$root/.githooks/pre-commit" "$root/.githooks/pre-push"
git config --local core.hooksPath .githooks
echo "SonarQube pre-commit and pre-push hooks enabled."
