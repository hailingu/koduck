#!/bin/sh
# Root-owned helper invoked by the wrapper via gosu: it alone loads the
# gate-only credentials from the mode-0600 files and execs the baked gate
# tooling. Never executed with PR-controlled arguments beyond the forwarded
# gate.py arguments.
set -eu
export KODUCK_SONAR_TOKEN
KODUCK_SONAR_TOKEN="$(cat /home/gate/.koduck/sonar-token)"
export KODUCK_AI_TEST_DATABASE_URL
KODUCK_AI_TEST_DATABASE_URL="$(cat /home/gate/.koduck/database-url)"
cd "$GITHUB_WORKSPACE"
exec python3 /opt/koduck-sonarqube/gate.py "$@"
