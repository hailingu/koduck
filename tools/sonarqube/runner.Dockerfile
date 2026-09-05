# Owner-authorized ephemeral CI worker; see README.md.
FROM ubuntu:24.04
ARG TARGETARCH
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git unzip python3 python3-venv build-essential \
    pkg-config libssl-dev libicu74 liblttng-ust1t64 libkrb5-3 zlib1g \
    openjdk-21-jre-headless socat xz-utils gosu sudo && \
    rm -rf /var/lib/apt/lists/* && \
    useradd --create-home --uid 1001 runner && \
    useradd --create-home --uid 1002 builder && \
    useradd --create-home --uid 1003 gate && \
    mkdir -p /home/builder/.cargo /home/builder/.npm && \
    chown -R builder:builder /home/builder
WORKDIR /home/runner
RUN case "$TARGETARCH" in amd64) arch=x64;; arm64) arch=arm64;; *) exit 1;; esac && \
    curl --fail --location --silent --show-error \
      "https://nodejs.org/dist/v22.11.0/node-v22.11.0-linux-${arch}.tar.xz" \
      | tar -xJ --strip-components=1 -C /usr/local && \
    mkdir actions && curl --fail --location --silent --show-error \
      "https://github.com/actions/runner/releases/download/v2.337.0/actions-runner-linux-${arch}-2.337.0.tar.gz" \
      | tar -xz -C actions && \
    curl --fail --location --silent --show-error \
      https://binaries.sonarsource.com/Distribution/sonar-scanner-cli/sonar-scanner-cli-7.3.0.5189.zip \
      -o /tmp/scanner.zip && unzip -q /tmp/scanner.zip -d /opt && rm /tmp/scanner.zip && \
    chown -R runner:runner /home/runner /opt/sonar-scanner-7.3.0.5189
USER runner
ENV PATH="/home/runner/.cargo/bin:/opt/sonar-scanner-7.3.0.5189/bin:${PATH}"
RUN curl --proto '=https' --tlsv1.2 --fail --silent --show-error https://sh.rustup.rs \
      -o /tmp/rustup.sh && sh /tmp/rustup.sh -y --default-toolchain 1.95.0 --profile minimal && \
    rm /tmp/rustup.sh && rustup component add rustfmt clippy llvm-tools-preview && \
    cargo install cargo-llvm-cov --version 0.9.0 --locked
# The gate tooling is baked into the image at build time from reviewed
# sources, so a pull request can never execute its own copy of gate.py with
# the analysis token; the checkout copy is only data (runtime venvs) to the
# baked boundary.
COPY --chown=root:root tools/sonarqube /opt/koduck-sonarqube
COPY --chown=root:root --chmod=0555 runner-entrypoint.sh /usr/local/bin/koduck-runner
COPY --chown=root:root --chmod=0555 runner-gate-wrapper.sh /usr/local/bin/koduck-gate
COPY --chown=root:root --chmod=0555 runner-gate-inner.sh /usr/local/bin/koduck-gate-inner
COPY --chown=root:root --chmod=0440 runner-gate-sudoers /etc/sudoers.d/koduck-gate
# The image default user stays the untrusted runner; the launcher overrides
# the container user to root so the entrypoint can prepare the gate-only
# credential files before dropping to the runner for the job itself.
ENTRYPOINT ["sh", "/usr/local/bin/koduck-runner"]
