#!/bin/sh
# Runs as root. The JIT config, the analysis token, and the fixture database
# URL arrive over stdin, never as container environment variables or command
# arguments; the secrets land in mode-0600 files owned by the `gate` user
# whose uid is the only one allowed to read them, and the Actions runner is
# started as the untrusted `runner` user with a scrubbed environment.
set -eu
IFS= read -r jit_config
IFS= read -r sonar_token
IFS= read -r database_url
mkdir -p /home/gate/.koduck
chown gate:gate /home/gate/.koduck
umask 077
printf "%s\n" "$sonar_token" > /home/gate/.koduck/sonar-token
printf "%s\n" "$database_url" > /home/gate/.koduck/database-url
chown gate:gate /home/gate/.koduck/sonar-token /home/gate/.koduck/database-url
socat TCP-LISTEN:9000,bind=127.0.0.1,reuseaddr,fork TCP:host.docker.internal:9000 &
cd /home/runner/actions
exec gosu runner env \
    HOME=/home/runner \
    PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/home/runner/.cargo/bin:/opt/sonar-scanner-7.3.0.5189/bin \
    ./run.sh --jitconfig "$jit_config"
