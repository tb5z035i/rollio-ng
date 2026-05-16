#!/usr/bin/env bash
# Ensure the host's apt config is set up to cross-install packages for
# TARGET_ARCH. Called from `make deps` before apt-get update/install;
# auto-skips for native builds (target == host).
#
# Usage: scripts/check-cross-apt.sh <target-arch>
#
# On arm64 cross builds, this script:
#   1. Registers TARGET_ARCH as a foreign architecture when needed.
#   2. Adds a Rollio-owned Ubuntu ports deb822 source when needed.
#   3. Checks that every /etc/apt/sources.list.d/* file carries an Architectures:
#      constraint (warn only -- third-party repos missing this cause
#      cosmetic 404 noise during `apt-get update` but do not break the
#      install).
#   4. Verifies a canary target package resolves when the apt cache is
#      already fresh enough to do so.

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

if [[ "$target_arch" != "arm64" ]]; then
    echo "unsupported cross TARGET_ARCH=${target_arch}; only arm64 setup is implemented" >&2
    exit 1
fi

codename="$(
    . /etc/os-release 2>/dev/null
    printf '%s' "${VERSION_CODENAME:-${UBUNTU_CODENAME:-}}"
)"
if [[ -z "$codename" ]]; then
    echo "failed to detect Ubuntu codename from /etc/os-release" >&2
    exit 1
fi

ports_uri="${ROLLIO_UBUNTU_PORTS_MIRROR:-http://ports.ubuntu.com/ubuntu-ports}"
ports_source="/etc/apt/sources.list.d/rollio-ubuntu-ports-${target_arch}.sources"
changed=0

# ── Preflight 1/4: foreign-arch registration ────────────────────────
if ! dpkg --print-foreign-architectures | grep -qx "$target_arch"; then
    echo "Registering ${target_arch} as a foreign dpkg architecture..."
    sudo dpkg --add-architecture "$target_arch"
    changed=1
fi

# ── Preflight 2/4: target apt source ────────────────────────────────
if ! {
    grep -RqsE "^[[:space:]]*Architectures:[[:space:]]+(.*[[:space:]])?${target_arch}([[:space:]]|$)" \
        /etc/apt/sources.list /etc/apt/sources.list.d 2>/dev/null \
    || grep -RqsE "^[[:space:]]*deb[[:space:]]+\[[^]]*arch=[^]]*${target_arch}([,[:space:]]|])" \
        /etc/apt/sources.list /etc/apt/sources.list.d 2>/dev/null
}; then
    echo "Writing ${ports_source} for Ubuntu ${codename} ${target_arch} packages..."
    sudo tee "$ports_source" >/dev/null <<EOF
Types: deb
URIs: ${ports_uri}
Suites: ${codename} ${codename}-updates ${codename}-backports ${codename}-security
Components: main restricted universe multiverse
Architectures: ${target_arch}
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
EOF
    changed=1
fi

# ── Preflight 3/4: Architectures: constraint scan (WARN only) ───────
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
some source serves ${target_arch} packages -- preflight 4/4 verifies).

EOF
    } >&2
fi

# ── Preflight 4/4: canary package resolves ──────────────────────────
# `libc6` is the most universal canary -- every Debian-derived system
# ships it. If it's not resolvable for the target arch, no source is
# actually serving target-arch packages.
#
# If this script just mutated apt configuration, the Makefile's next step
# is `sudo apt-get update`, so the cache is expected to be stale here.
if (( changed == 0 )) && ! apt-cache show "libc6:${target_arch}" >/dev/null 2>&1; then
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
