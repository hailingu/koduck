#!/bin/sh
# The JIT config, the analysis token, and the fixture database URL arrive
# over stdin, never as container environment variables or command arguments.
# The token is stored in a mode-0600 file for the gate step alone and is
# never exported, so job steps do not inherit any Sonar credential.
set -eu
IFS= read -r jit_config
IFS= read -r sonar_token
IFS= read -r database_url
mkdir -p "$HOME/.koduck"
umask 077
printf "%s\n" "$sonar_token" > "$HOME/.koduck/sonar-token"
printf "%s\n" "$database_url" > "$HOME/.koduck/database-url"
socat TCP-LISTEN:9000,bind=127.0.0.1,reuseaddr,fork TCP:host.docker.internal:9000 &
cd /home/runner/actions
exec ./run.sh --jitconfig "$jit_config"
