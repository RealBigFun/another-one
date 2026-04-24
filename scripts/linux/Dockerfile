# Builder image for AnotherOne's Linux AppImage.
#
# Ubuntu 22.04 base pins GLIBC 2.35, which gives the bundled binary
# broad distro compatibility (every mainstream Linux desktop from
# mid-2022 onward). Rust is pinned to 1.92 — matching the local
# toolchain at the time this was written — for reproducibility.
#
# Built and invoked by scripts/linux/build-in-container.sh; not meant
# to be run directly. The image has no entrypoint because callers
# pass `cargo build …` as the command (and the target directory is
# volume-mounted, not baked in).

FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# Force apt to IPv4. Docker's default bridge network on most Linux
# hosts gives containers AAAA records (archive.ubuntu.com resolves to
# IPv6) but no IPv6 route out, which hangs apt-get update. Host-
# level Docker IPv6 is a separate config story; pinning apt to IPv4
# sidesteps it.
RUN echo 'Acquire::ForceIPv4 "true";' > /etc/apt/apt.conf.d/99-force-ipv4

RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential \
      ca-certificates \
      cmake \
      curl \
      git \
      pkg-config \
      libssl-dev \
      libxkbcommon-dev \
      libxkbcommon-x11-dev \
      libxcb1-dev \
      libxcb-xkb-dev \
      libxcb-composite0-dev \
      libxcb-render0-dev \
      libxcb-shape0-dev \
      libxcb-xfixes0-dev \
      libwayland-dev \
      libfontconfig1-dev \
      libfreetype-dev \
    && rm -rf /var/lib/apt/lists/*

ARG RUST_VERSION=1.92.0
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain "$RUST_VERSION" --profile minimal --no-modify-path \
    && chmod -R a+rwX /opt/rustup /opt/cargo

WORKDIR /src
