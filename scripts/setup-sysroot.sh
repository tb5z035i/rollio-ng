#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREBUILT="$REPO_ROOT/prebuilt"
SYSROOT="$REPO_ROOT/.sysroot"
STAMP="$SYSROOT/.stamp"

# Idempotency: skip if stamp is newer than all prebuilt archives.
if [[ -f "$STAMP" ]]; then
    needs_update=false
    for f in "$PREBUILT"/*.deb "$PREBUILT"/cora_*.tar.gz; do
        [[ -e "$f" ]] || continue
        if [[ "$f" -nt "$STAMP" ]]; then
            needs_update=true
            break
        fi
    done
    if [[ "$needs_update" == "false" ]]; then
        echo "sysroot: up to date (stamp newer than all prebuilt archives)"
        exit 0
    fi
fi

echo "sysroot: populating .sysroot/ from prebuilt/ ..."
rm -rf "$SYSROOT"

for triple in x86_64-linux-gnu aarch64-linux-gnu; do
    mkdir -p "$SYSROOT/$triple/usr/lib" "$SYSROOT/$triple/usr/include" "$SYSROOT/$triple/usr/lib/cmake"
done

arch_for_triple() {
    case "$1" in
        x86_64-linux-gnu)  echo "amd64" ;;
        aarch64-linux-gnu) echo "aarch64" ;;
    esac
}

triple_for_deb_arch() {
    case "$1" in
        amd64) echo "x86_64-linux-gnu" ;;
        arm64) echo "aarch64-linux-gnu" ;;
        *)     echo "" ;;
    esac
}

triple_for_tar_arch() {
    case "$1" in
        x86_64)  echo "x86_64-linux-gnu" ;;
        aarch64) echo "aarch64-linux-gnu" ;;
        *)       echo "" ;;
    esac
}

# --- Extract .deb packages ---
for deb in "$PREBUILT"/*.deb; do
    [[ -e "$deb" ]] || continue
    base="$(basename "$deb")"
    # Infer arch from _<arch>.deb suffix
    deb_arch="${base##*_}"
    deb_arch="${deb_arch%.deb}"
    triple="$(triple_for_deb_arch "$deb_arch")"
    if [[ -z "$triple" ]]; then
        echo "  skip (unknown arch): $base"
        continue
    fi
    echo "  deb: $base → .sysroot/$triple/"
    dpkg-deb -x "$deb" "$SYSROOT/$triple"
done

# --- Extract cora-sdk tarballs (nested archive) ---
for outer in "$PREBUILT"/cora_*.tar.gz; do
    [[ -e "$outer" ]] || continue
    base="$(basename "$outer")"
    # Infer arch from cora_<arch>.tar.gz
    tar_arch="${base#cora_}"
    tar_arch="${tar_arch%.tar.gz}"
    triple="$(triple_for_tar_arch "$tar_arch")"
    if [[ -z "$triple" ]]; then
        echo "  skip (unknown arch): $base"
        continue
    fi

    echo "  cora: $base → .sysroot/$triple/"
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    # Outer tarball contains the inner cora-sdk tarball + other files
    tar xzf "$outer" -C "$tmpdir"
    inner="$(find "$tmpdir" -name 'cora-sdk_*_linux_*.tar.gz' | head -1)"
    if [[ -z "$inner" ]]; then
        echo "    warning: no nested cora-sdk tarball found in $base"
        rm -rf "$tmpdir"
        continue
    fi

    # Extract inner tarball
    tar xzf "$inner" -C "$tmpdir"
    # Find the extracted cora-sdk directory (contains opt/cora/)
    sdk_dir="$(find "$tmpdir" -maxdepth 1 -type d -name 'cora-sdk_*' | head -1)"
    if [[ -z "$sdk_dir" ]]; then
        echo "    warning: no cora-sdk directory found after extraction"
        rm -rf "$tmpdir"
        continue
    fi

    # Relocate opt/cora/{lib,include} → usr/{lib,include}
    if [[ -d "$sdk_dir/opt/cora/lib" ]]; then
        cp -a "$sdk_dir/opt/cora/lib/." "$SYSROOT/$triple/usr/lib/"
    fi
    if [[ -d "$sdk_dir/opt/cora/include" ]]; then
        cp -a "$sdk_dir/opt/cora/include/." "$SYSROOT/$triple/usr/include/"
    fi

    rm -rf "$tmpdir"
    trap - EXIT
done

# --- Create unversioned .so symlinks where missing ---
find "$SYSROOT" -name '*.so.*' | while read -r versioned; do
    # e.g. libdataloop.so.2.0.1 → libdataloop.so
    lib_dir="$(dirname "$versioned")"
    filename="$(basename "$versioned")"
    # Strip all version suffixes: libfoo.so.1.2.3 → libfoo.so
    bare="${filename%%.*}.so"
    if [[ ! -e "$lib_dir/$bare" ]]; then
        ln -sf "$filename" "$lib_dir/$bare"
        echo "  symlink: $bare → $filename"
    fi
done

touch "$STAMP"
echo "sysroot: done."
