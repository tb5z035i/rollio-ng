#!/usr/bin/env bash
# Validate that the host's apt config is set up to cross-install
# packages for TARGET_ARCH. Called from `make deps` before any
# apt-get invocation; auto-skips for native builds (target == host).
#
# Usage: scripts/check-cross-apt.sh <target-arch>
#
# On cross builds, runs three checks:
#   1. TARGET_ARCH is a registered foreign architecture (hard fail).
#   2. Every /etc/apt/sources.list.d/* file carries an Architectures:
#      constraint (warn only -- third-party repos missing this cause
#      cosmetic 404 noise during `apt-get update` but do not break the
#      install).
#   3. A canary <target> package resolves through the configured
#      sources (hard fail).
#
# This script deliberately does NOT mutate apt config. Adding a foreign
# architecture and wiring up sources.list.d/ entries is a host-policy
# decision left to the operator. On a hard-fail, the printed message
# carries the exact sudo commands the operator can copy-paste.

set -euo pipefail

if [[ $# -lt 1 || -z "${1:-}" ]]; then
    echo "usage: $0 <target-arch>" >&2
    exit 2
fi

target_arch="$1"
host_arch="$(dpkg --print-architecture 2>/dev/null || echo unknown)"

# Native build -- nothing to verify; let the recipe proceed.
if [[ "$target_arch" == "$host_arch" ]]; then
    exit 0
fi

# ── Preflight 1/3: foreign-arch registration ────────────────────────
if ! dpkg --print-foreign-architectures | grep -qx "$target_arch"; then
    cat <<EOF >&2

make deps TARGET_ARCH=${target_arch}: ${target_arch} is not a registered foreign architecture.

Run these once on this host (NOT done by make):

  sudo dpkg --add-architecture ${target_arch}
  # then drop a sources file for ports.ubuntu.com (or a mirror), e.g.:
  sudo tee /etc/apt/sources.list.d/ubuntu-ports.sources >/dev/null <<'PORTSEOF'
  Types: deb
  URIs: http://ports.ubuntu.com/ubuntu-ports
  Suites: noble noble-updates noble-backports noble-security
  Components: main restricted universe multiverse
  Architectures: ${target_arch}
  Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
  PORTSEOF
  sudo apt-get update

EOF
    exit 1
fi

# ── Preflight 2/3: Architectures: constraint scan (WARN only) ───────
# Without per-source Architectures:, apt fans out every registered arch
# over every source -- main archive 404s on arm64, ports 404s on amd64,
# many third-party repos only ship one arch. This is cosmetic noise,
# not a functional failure, so we warn but do not abort.
bad=()
while IFS= read -r f; do
    bad+=("$f")
done < <(
    for f in /etc/apt/sources.list.d/*.sources; do
        [[ -f "$f" ]] || continue
        if grep -qE '^Types:[[:space:]]+deb([[:space:]]|$)' "$f" \
           && ! grep -qE '^Architectures:' "$f"; then
            echo "$f"
        fi
    done
    for f in /etc/apt/sources.list.d/*.list /etc/apt/sources.list; do
        [[ -f "$f" ]] || continue
        if grep -qE '^[[:space:]]*deb[[:space:]]+(\[[^]]*\][[:space:]]+)?https?://' "$f" \
           && ! grep -qE '^[[:space:]]*deb[[:space:]]+\[[^]]*arch=' "$f"; then
            echo "$f"
        fi
    done
)
if (( ${#bad[@]} > 0 )); then
    {
        echo
        echo "WARNING: apt sources missing 'Architectures:' constraint:"
        printf '  %s\n' "${bad[@]}"
        cat <<EOF

These will emit 404 noise during 'apt-get update' (one arch per source
that the upstream does not serve). To silence, constrain each source to
the arch its mirror actually serves, e.g.:

  # main archive (Ubuntu's mirror) -> amd64 only:
  sudo sed -i '/^Types: deb\$/a Architectures: amd64' \\
      /etc/apt/sources.list.d/ubuntu.sources
  # ports archive -> ${target_arch} only:
  sudo sed -i '/^Types: deb\$/a Architectures: ${target_arch}' \\
      /etc/apt/sources.list.d/ubuntu-ports.sources
  sudo apt-get update

Continuing without fixing (the install below will still work as long as
some source serves ${target_arch} packages -- preflight 3/3 verifies).

EOF
    } >&2
fi

# ── Preflight 3/3: canary package resolves ──────────────────────────
# `libc6` is the most universal canary -- every Debian-derived system
# ships it. If it's not resolvable for the target arch, no source is
# actually serving target-arch packages.
if ! apt-cache show "libc6:${target_arch}" >/dev/null 2>&1; then
    cat <<EOF >&2

make deps TARGET_ARCH=${target_arch}: ${target_arch} is registered, but
libc6:${target_arch} is not resolvable. Run \`sudo apt-get update\`
and verify that a ${target_arch}-serving source (e.g. ubuntu-ports for
arm64) is configured under /etc/apt/sources.list.d/ with the right
Architectures: line.

EOF
    exit 1
fi

exit 0
