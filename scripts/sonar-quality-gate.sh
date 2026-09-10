#!/bin/sh
# Shared hook entry point, adapted from PlotWeave/scripts/sonar-quality-gate.sh.
set -eu
script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(CDPATH= cd -- "$script_directory/.." && pwd)
cd "$repository_root"
if [ "$#" -eq 0 ]; then set -- check; fi
# Prefer Koduck's own credential; never accidentally reuse another project's SONAR_TOKEN.
# Interactive zsh reads ~/.zshrc when a GUI Git client did not inherit its exports.
if [ -z "${KODUCK_SONAR_TOKEN:-}" ]; then
  command -v zsh >/dev/null 2>&1 || { echo 'SONAR_TOKEN_MISSING: export KODUCK_SONAR_TOKEN' >&2; exit 1; }
  exec zsh -ic 'set +x; : "${KODUCK_SONAR_TOKEN:?Export Koduck token in ~/.zshrc}"; exec python3 "$@"' \
    koduck-sonar "$repository_root/tools/sonarqube/gate.py" "$@"
fi
exec python3 "$repository_root/tools/sonarqube/gate.py" "$@"
