# A light dev container for reth: the Rust toolchains, LLVM, lld and
# cargo-nextest. See devcontainer.json.
FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# LLVM 22 through the same script the release Dockerfile uses: the default
# `jit` feature links revmc against it via llvm-sys. The script also installs
# clang and lld (the linker on aarch64; devcontainer.json explains why). The
# remaining packages match the release Dockerfile: m4 builds GMP for the `gmp`
# feature, libclang-dev and pkg-config serve the bindgen and C dependencies.
COPY .github/scripts/install_llvm_ubuntu.sh /tmp/install_llvm.sh
RUN /tmp/install_llvm.sh \
    && rm /tmp/install_llvm.sh \
    && apt-get install -y --no-install-recommends libclang-dev m4 pkg-config \
    && rm -rf /var/lib/apt/lists/*

# reth pins no toolchain file; CI builds on stable and checks the MSRV
# (`rust-version` in Cargo.toml) separately. Install the current stable into
# the image, so the first `cargo` call in the container does not download it,
# plus nightly rustfmt for `make fmt`. rust-src is added on top: rust-analyzer
# needs the standard library's source to resolve std.
# Installing as root leaves the new toolchains in group root; hand them to the
# rustlang group the vscode user is in, so rustup works without sudo later.
RUN rustup toolchain install stable --profile minimal --component clippy,rustfmt,rust-src \
    && rustup toolchain install nightly --profile minimal --component rustfmt \
    && rustup default stable \
    && chgrp -R rustlang "$RUSTUP_HOME" "$CARGO_HOME" \
    && chmod -R g+w "$RUSTUP_HOME" "$CARGO_HOME"

# cargo-nextest, the test runner CI uses, at the version CI pins. The prebuilt
# binary, since building it from source adds minutes to every image build.
RUN case "$(dpkg --print-architecture)" in \
        amd64) platform=linux ;; \
        arm64) platform=linux-arm ;; \
        *) echo "unsupported architecture" >&2; exit 1 ;; \
    esac \
    && curl -LsSf "https://get.nexte.st/0.9.140/$platform" | tar zxf - -C /usr/local/bin

# cargo-hack, at the version CI pins.
RUN cargo install cargo-hack@0.6.45 --locked
