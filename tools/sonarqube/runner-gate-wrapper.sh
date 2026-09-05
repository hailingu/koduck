#!/bin/sh
# Executed by the workflow's sonar step through the runner->root sudoers
# rule. Prepares the write boundaries, then runs the BAKED (image-pinned,
# repository-reviewed) gate tooling as the `gate` user, whose home holds the
# mode-0600 analysis token no PR-controlled step or subprocess can read.
# Arguments are forwarded verbatim to `gate.py check`.
set -eu
: "${GITHUB_WORKSPACE:?must run inside a GitHub Actions job}"
cd "$GITHUB_WORKSPACE"

# The scanner writes evidence under the Git common directory and build
# artifacts into the workspace; those paths become writable by the exact
# users that need them and nothing else.
mkdir -p .git/sonarqube target
chown -R gate:gate .git/sonarqube
chown -R builder:builder target tools/governance-validator tools/sonarqube

BUILDER_UID="$(id -u builder)"
BUILDER_GID="$(id -g builder)"
export KODUCK_SONAR_BUILDER_UID
export KODUCK_SONAR_BUILDER_GID
export KODUCK_SONAR_RUNTIME_TOOLS="$GITHUB_WORKSPACE/tools/sonarqube"
export RUSTUP_HOME="/home/runner/.rustup"
export CARGO_HOME="/home/builder/.cargo"
export NPM_CONFIG_CACHE="/home/builder/.npm"
export PATH="/home/runner/.cargo/bin:/opt/sonar-scanner-7.3.0.5189/bin:$PATH"

exec gosu gate /usr/local/bin/koduck-gate-inner "$@"
