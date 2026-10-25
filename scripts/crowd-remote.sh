#!/usr/bin/env bash
# Runs the simulated guests of the load test (`crowd guests`) on a second machine, in a Docker container with limited CPU and memory.
# The host (`crowd_host`, see context/mcp-remote/test-charge.md) stays on this computer.
#
#   scripts/crowd-remote.sh build            copy the sources (core, session, net, crowd) and compile them in `rust:1.99` on the server
#   scripts/crowd-remote.sh run LINK_FILE VIEW_LINK_FILE [options of `crowd guests`...]
#                                            start the guests in a container; their reports come back to out/remote/
#   scripts/crowd-remote.sh clean            remove the sources, the build folder and (REMOVE_IMAGE=1) the rust image
#
# Environment: SSH_HOST (default homelab-ts), CPUS (4), MEMORY (6g), NETEM="delay 40ms loss 1%" (needs NET_ADMIN in the container: added
# automatically when set; the container's network is then degraded on its way out).
set -eu
SSH_HOST="${SSH_HOST:-homelab-ts}"
CPUS="${CPUS:-4}"
MEMORY="${MEMORY:-6g}"
REMOTE_DIR="crowd-load"
cd "$(dirname "$0")/.."

case "${1:-}" in
  build)
    work=out/remote-src
    rm -rf "$work"
    mkdir -p "$work"
    cp Cargo.lock rustfmt.toml "$work"/
    sed 's#^members = .*#members = ["crates/inkboard-core", "crates/inkboard-session", "crates/inkboard-net", "crates/inkboard-crowd"]#' Cargo.toml > "$work/Cargo.toml"
    ssh "$SSH_HOST" "rm -rf $REMOTE_DIR/src && mkdir -p $REMOTE_DIR/src $REMOTE_DIR/target $REMOTE_DIR/out"
    # the workspace manifest of the slice goes next to the crates
    tar -czf out/crowd-slice.tgz --exclude=target -C . crates/inkboard-core crates/inkboard-session crates/inkboard-net crates/inkboard-crowd
    ssh "$SSH_HOST" "tar -xz -C $REMOTE_DIR/src" < out/crowd-slice.tgz
    scp -q "$work/Cargo.toml" "$work/Cargo.lock" "$work/rustfmt.toml" "$SSH_HOST:$REMOTE_DIR/src/"
    ssh "$SSH_HOST" "docker run --rm --cpus $CPUS --memory $MEMORY -v \$HOME/$REMOTE_DIR/src:/src -v \$HOME/$REMOTE_DIR/target:/target -e CARGO_TARGET_DIR=/target -w /src rust:1.99 cargo build --release -p inkboard-crowd 2>&1 | tail -5"
    ;;
  run)
    link="${2:?edit link file}"
    view="${3:?view link file}"
    shift 3
    scp -q "$link" "$SSH_HOST:$REMOTE_DIR/out/edit.link"
    scp -q "$view" "$SSH_HOST:$REMOTE_DIR/out/view.link"
    caps=""
    pre=""
    if [ -n "${NETEM:-}" ]; then
      caps="--cap-add NET_ADMIN"
      pre="apt-get update -qq >/dev/null 2>&1 && apt-get install -y -qq iproute2 >/dev/null 2>&1; tc qdisc add dev eth0 root netem ${NETEM}; "
    fi
    ssh "$SSH_HOST" "docker run --rm --name crowd-guests $caps --cpus $CPUS --memory $MEMORY -v \$HOME/$REMOTE_DIR/target:/target:ro -v \$HOME/$REMOTE_DIR/out:/out rust:1.99 bash -c '${pre}/target/release/crowd guests --ticket-file /out/edit.link --view-ticket-file /out/view.link --out /out $*'"
    mkdir -p out/remote
    scp -q "$SSH_HOST:$REMOTE_DIR/out/guests-*" out/remote/ || true
    ;;
  clean)
    ssh "$SSH_HOST" "rm -rf $REMOTE_DIR; docker rm -f crowd-guests >/dev/null 2>&1 || true"
    if [ "${REMOVE_IMAGE:-0}" = 1 ]; then ssh "$SSH_HOST" "docker rmi rust:1.99"; fi
    ;;
  *)
    sed -n '2,13p' "$0"
    exit 2
    ;;
esac
