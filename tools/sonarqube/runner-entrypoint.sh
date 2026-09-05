#!/bin/sh
# One-job JIT config arrives over stdin, never in docker command arguments.
set -eu
IFS= read -r jit_config
socat TCP-LISTEN:9000,bind=127.0.0.1,reuseaddr,fork TCP:host.docker.internal:9000 &
cd /home/runner/actions
exec ./run.sh --jitconfig "$jit_config"
