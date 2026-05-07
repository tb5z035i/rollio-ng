#!/usr/bin/env bash
# Pack already-built artifacts into a Debian package and a Python wheel.
#
#   ./build.sh [all|core|nero|clean]
#
# `all` (the default) produces two artifacts in $DEB_DIST (default: dist/):
#   * rollio_<ver>_<arch>.deb              all Rust binaries (incl. encoder) + UI bundles
#   * rollio_device_nero-<ver>-py3-none-any.whl  Nero hardware driver wheel
#
# This script does NOT compile. Run `make build` first (or `make package-all`).
#
# Env overrides:
#   DEB_VERSION         package version (default: 1.0.0-1)
#   DEB_ARCH            dpkg architecture (default: dpkg --print-architecture)
#   DEB_DIST            output directory (default: dist)
#   STAGING             staging tree   (default: .deb-staging)
#   TARGET_DIR          cargo profile target dir (default: target/release)
#   CAMERAS_BUILD_DIR   cmake build dir for C++ camera drivers
#                       (default: cameras/build)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

DEB_VERSION="${DEB_VERSION:-1.0.0-1}"
DEB_ARCH="${DEB_ARCH:-$(dpkg --print-architecture 2>/dev/null || echo amd64)}"
DEB_DIST="${DEB_DIST:-dist}"
STAGING="${STAGING:-.deb-staging}"
TARGET_DIR="${TARGET_DIR:-target/release}"
CAMERAS_BUILD_DIR="${CAMERAS_BUILD_DIR:-cameras/build}"

# Binaries omitted from dpkg-shlibdeps (still shipped). Encoder links the full
# FFmpeg stack; Depends are not generated for it until packaging is finalized.
# rollio-device-realsense is *not* on this list: librealsense2 is built into
# the binary statically (see cameras/CMakeLists.txt), so the only remaining
# runtime deps are libusb-1.0 and libudev which are normal Ubuntu apt
# packages that dpkg-shlibdeps resolves cleanly.
SHLIBDEPS_EXCLUDE_BINS=(
    rollio-encoder
)

# All Rust binaries shipped in /usr/bin (encoder included for shipping).
CORE_BINS=(
    rollio
    rollio-encoder
    rollio-visualizer
    rollio-control-server
    rollio-ui-server
    rollio-teleop-router
    rollio-episode-assembler
    rollio-storage
    rollio-monitor
    rollio-device-pseudo
    rollio-device-airbot-play
    rollio-device-v4l2
    rollio-bus-tap
    rollio-test-publisher
)

# C++ camera-driver binaries shipped in /usr/bin. Each entry is the path of
# the built executable relative to $CAMERAS_BUILD_DIR; the basename is what
# lands under /usr/bin/. The controller's runtime path resolution looks for
# these in `cameras/build/<dir>/` during in-tree development and on $PATH
# (i.e. /usr/bin/) once the .deb is installed -- see
# controller/src/runtime_paths.rs::resolve_registered_program. Add another
# `<subdir>/<binary>` entry here when cameras/<driver>/CMakeLists.txt grows
# a new add_executable(rollio-device-...).
#
# Note: rollio-device-pseudo-camera is intentionally omitted -- it is built
# only to back the C++ integration tests in cameras/pseudo/tests and is not
# discoverable by the controller (see cameras/README.md).
CAMERA_BINS=(
    "realsense/rollio-device-realsense"
    # rollio-device-umi: FastDDS->iceoryx2 bridge for cora's H264 cameras
    # and IMU streams. Built under devices/umi/ but staged from the same
    # cmake build tree as cameras/, so the path mirrors the realsense one.
    "devices/umi/rollio-device-umi"
)

CORE_STAGING="$STAGING/rollio"

# debian/ is the static deb root template. It carries DEBIAN/ metadata and
# the AIRBOT host-setup payload (bin/, lib/udev/, lib/systemd/) -- same
# content as third_party/airbot-play-rust/root/ but vendored so packaging
# is self-contained. build.sh copies this whole tree into the staging
# directory, then layers the freshly-built Rust binaries + UI bundles
# on top. control.in carries @DEB_VERSION@ / @DEB_ARCH@ / @SHLIBS@
# placeholders that get substituted at pack time.
DEBIAN_TEMPLATE_DIR="debian"

log()  { printf '\033[1;34m[build.sh]\033[0m %s\n' "$*" >&2; }
warn() { printf '\033[1;33m[build.sh]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[build.sh]\033[0m %s\n' "$*" >&2; exit 1; }

require_cmd() {
    local cmd="$1" hint="${2:-}"
    command -v "$cmd" >/dev/null 2>&1 || die "missing required tool: $cmd${hint:+ ($hint)}"
}

preflight_deb() {
    require_cmd dpkg-deb       "apt install dpkg-dev"
    require_cmd dpkg-shlibdeps "apt install dpkg-dev"
    require_cmd file           "apt install file"
}

preflight_wheel() {
    if command -v uv >/dev/null 2>&1; then
        WHEEL_BUILDER=(uv build --wheel)
    elif python3 -c 'import build' >/dev/null 2>&1; then
        WHEEL_BUILDER=(python3 -m build --wheel)
    else
        die "need uv (preferred) or python3-build for the Nero wheel; try \`pipx install uv\`"
    fi
}

assert_built() {
    for b in "${CORE_BINS[@]}"; do
        [[ -x "$TARGET_DIR/$b" ]] || die "missing $TARGET_DIR/$b -- run \`make build\` (or \`make package-all\`) first"
    done
    for entry in "${CAMERA_BINS[@]}"; do
        [[ -x "$CAMERAS_BUILD_DIR/$entry" ]] \
            || die "missing $CAMERAS_BUILD_DIR/$entry -- run \`make cpp-build\` (or \`make build\`) first"
    done
    [[ -d ui/web/dist ]]      || die "missing ui/web/dist -- run \`make ui-build\` (or \`make build\`) first"
    [[ -d ui/terminal/dist ]] || die "missing ui/terminal/dist -- run \`make ui-build\` (or \`make build\`) first"
    [[ -f ui/terminal/dist/index.js ]] \
        || die "missing ui/terminal/dist/index.js -- run \`make ui-build\` (or \`make build\`) first"
    [[ -f ui/terminal/dist/native-rust.worker.js ]] \
        || die "missing ui/terminal/dist/native-rust.worker.js -- run \`make ui-build\` (or \`make build\`) first"
    [[ -f ui/terminal/dist/package.json ]] \
        || die "missing ui/terminal/dist/package.json (ESM marker) -- run \`make ui-build\` (or \`make build\`) first"
    [[ -f ui/terminal/native/rollio-native-ascii.node ]] \
        || die "missing ui/terminal/native/rollio-native-ascii.node -- run \`make ui-build\` (or \`make build\`) first"
    # bundle-terminal.mjs vendors sharp + its runtime deps (incl. the per-arch
    # @img/sharp-* native binding npm picked locally) into .deb-vendor/.
    [[ -f ui/terminal/.deb-vendor/node_modules/sharp/package.json ]] \
        || die "missing ui/terminal/.deb-vendor/node_modules/sharp -- run \`make ui-build\` (or \`make build\`) first"
    compgen -G "ui/terminal/.deb-vendor/node_modules/@img/sharp-*" >/dev/null \
        || die "missing ui/terminal/.deb-vendor/node_modules/@img/sharp-* -- run \`cd ui/terminal && npm install\` on the target arch and rebuild"
}

run_shlibdeps() {
    # $1 = staging root, $2 = substvars output path, $3 = package name
    # Limit shlibdeps to /usr/bin/ ELFs only (no vendored Python trees).
    # dpkg-shlibdeps insists on reading debian/control from CWD; synthesize a
    # minimal one per package in a temp dir under $STAGING and run from there.
    local root="$1" subst="$2" pkg="$3"
    local subst_abs root_abs ctldir
    subst_abs="$(realpath -m "$subst")"
    root_abs="$(realpath "$root")"
    ctldir="$(realpath "$STAGING")/.shlibdeps-$pkg"
    rm -rf "$ctldir"
    install -d "$ctldir/debian"
    cat > "$ctldir/debian/control" <<EOF
Source: $pkg
Section: video
Priority: optional
Maintainer: Rollio Maintainers <rollio@localhost>

Package: $pkg
Architecture: any
Description: shlibdeps stub for $pkg
 Synthetic control file used only by build.sh to satisfy dpkg-shlibdeps.
EOF
    rm -f "$subst_abs"
    local elfs=() exclude name
    while IFS= read -r -d '' f; do
        name="$(basename "$f")"
        local skip=0
        for exclude in "${SHLIBDEPS_EXCLUDE_BINS[@]}"; do
            if [[ "$name" == "$exclude" ]]; then
                skip=1
                break
            fi
        done
        [[ "$skip" -eq 1 ]] && continue
        if file -b "$f" | grep -qE 'ELF.*(executable|shared object)'; then
            elfs+=("$(realpath "$f")")
        fi
    done < <(find "$root_abs/usr/bin" -maxdepth 1 -type f -print0 2>/dev/null)
    [[ ${#elfs[@]} -gt 0 ]] || die "no ELFs found under $root/usr/bin for shlibdeps"
    ( cd "$ctldir" && dpkg-shlibdeps -T"$subst_abs" -pshlibs "${elfs[@]}" )
}

extract_shlibs_depends() {
    grep '^shlibs:Depends=' "$1" 2>/dev/null | head -1 | cut -d= -f2-
}

stage_debian_template() {
    # $1 = staging root, $2 = computed shlibs:Depends value
    # Copies debian/ wholesale into the staging root, then renders
    # DEBIAN/control.in -> DEBIAN/control with @DEB_VERSION@,
    # @DEB_ARCH@, @SHLIBS@ filled in. Edit files under debian/ to
    # change anything packaged here -- this script no longer carries
    # heredoc-generated payload.
    local root="$1" shlibs="$2"
    [[ -d "$DEBIAN_TEMPLATE_DIR" ]] || die "missing $DEBIAN_TEMPLATE_DIR/ template"
    [[ -d "$DEBIAN_TEMPLATE_DIR/DEBIAN" ]] || die "missing $DEBIAN_TEMPLATE_DIR/DEBIAN/"
    [[ -f "$DEBIAN_TEMPLATE_DIR/DEBIAN/control.in" ]] \
        || die "missing $DEBIAN_TEMPLATE_DIR/DEBIAN/control.in"

    log "Copying $DEBIAN_TEMPLATE_DIR/ template into $root"
    # `cp -aT` copies contents-of-source into dest while preserving modes
    # (postinst/postrm stay 0755, .rules / .service stay 0644).
    cp -aT "$DEBIAN_TEMPLATE_DIR" "$root"

    # README in the template is for humans editing debian/, not for the
    # installed package. Strip it (and the control template) from staging.
    rm -f "$root/README.md" "$root/DEBIAN/control.in"

    # Substitute placeholders. awk handles values containing `/`, `&`, etc.
    # without the sed escaping pitfalls.
    awk \
        -v ver="$DEB_VERSION" \
        -v arch="$DEB_ARCH" \
        -v shlibs="$shlibs" \
        '{
            gsub(/@DEB_VERSION@/, ver);
            gsub(/@DEB_ARCH@/, arch);
            gsub(/@SHLIBS@/, shlibs);
            print;
         }' "$DEBIAN_TEMPLATE_DIR/DEBIAN/control.in" > "$root/DEBIAN/control"
    chmod 0644 "$root/DEBIAN/control"
}

build_core() {
    preflight_deb
    assert_built
    log "Staging rollio -> $CORE_STAGING (template: $DEBIAN_TEMPLATE_DIR/)"
    rm -rf "$CORE_STAGING"
    install -d "$CORE_STAGING/usr/bin" \
               "$CORE_STAGING/usr/share/rollio/ui/web" \
               "$CORE_STAGING/usr/share/rollio/ui/terminal"
    for b in "${CORE_BINS[@]}"; do
        install -m755 "$TARGET_DIR/$b" "$CORE_STAGING/usr/bin/"
    done
    # C++ camera binaries land alongside the Rust ones so the controller
    # finds them on $PATH on installed systems. dpkg-shlibdeps below picks
    # them up automatically (it scans usr/bin/ for ELFs); if a build host
    # has librealsense2 installed, the realsense binary will pull in the
    # corresponding Depends -- without it, the binary is a stub (compile
    # guarded on ROLLIO_HAVE_REALSENSE) but still ships under the same
    # name so `rollio setup` discovery doesn't blow up.
    for entry in "${CAMERA_BINS[@]}"; do
        install -m755 "$CAMERAS_BUILD_DIR/$entry" "$CORE_STAGING/usr/bin/"
    done
    cp -a ui/web/dist      "$CORE_STAGING/usr/share/rollio/ui/web/dist"
    cp -a ui/terminal/dist "$CORE_STAGING/usr/share/rollio/ui/terminal/dist"

    # The terminal UI bundle keeps `sharp` external (native addon) and uses a
    # native ASCII N-API addon. Both must sit next to dist/ in the install
    # tree so Node resolves them at runtime:
    #   /usr/share/rollio/ui/terminal/native/rollio-native-ascii.node
    #   /usr/share/rollio/ui/terminal/node_modules/{sharp,@img/sharp-*,...}/
    # The vendor tree (sharp + its runtime closure, incl. per-arch @img/sharp-*
    # native bindings) is staged by ui/terminal/scripts/bundle-terminal.mjs.
    install -d "$CORE_STAGING/usr/share/rollio/ui/terminal/native"
    install -m644 ui/terminal/native/rollio-native-ascii.node \
        "$CORE_STAGING/usr/share/rollio/ui/terminal/native/"
    cp -a ui/terminal/.deb-vendor/node_modules \
        "$CORE_STAGING/usr/share/rollio/ui/terminal/node_modules"

    local subst="$STAGING/substvars-rollio"
    log "Computing rollio Depends via dpkg-shlibdeps"
    run_shlibdeps "$CORE_STAGING" "$subst" rollio
    local shlibs
    shlibs="$(extract_shlibs_depends "$subst")"
    [[ -n "$shlibs" ]] || die "dpkg-shlibdeps produced no Depends for rollio"

    stage_debian_template "$CORE_STAGING" "$shlibs"

    install -d "$DEB_DIST"
    local out="$DEB_DIST/rollio_${DEB_VERSION}_${DEB_ARCH}.deb"
    log "Building $out"
    dpkg-deb --root-owner-group --build "$CORE_STAGING" "$out" >/dev/null
    printf '%s\n' "$out"
}

build_nero() {
    preflight_wheel
    [[ -f robots/nero/pyproject.toml ]] || die "robots/nero/pyproject.toml not found"
    install -d "$DEB_DIST"
    log "Building Nero wheel via ${WHEEL_BUILDER[*]}"
    # `uv build` and `python -m build` both accept a project directory.
    "${WHEEL_BUILDER[@]}" --out-dir "$DEB_DIST" robots/nero >&2
    # Print resulting wheel paths (newest match wins for matching name).
    find "$DEB_DIST" -maxdepth 1 -name 'rollio_device_nero-*.whl' -printf '%p\n' | sort
}

clean() {
    log "Removing $STAGING and $DEB_DIST"
    rm -rf "$STAGING" "$DEB_DIST"
}

cmd="${1:-all}"
case "$cmd" in
    core)
        out="$(build_core)"
        log "Done: $out"
        ;;
    nero)
        outs="$(build_nero)"
        log "Done:"
        printf '  %s\n' $outs >&2
        ;;
    all)
        c="$(build_core)"
        n="$(build_nero)"
        log "All artifacts:"
        printf '  %s\n' "$c" $n >&2
        ;;
    clean)
        clean
        ;;
    -h|--help|help)
        awk '/^#!/ {next} /^#/ {sub(/^# ?/,""); print; next} {exit}' "${BASH_SOURCE[0]}"
        ;;
    *)
        die "unknown subcommand: $cmd (try: all|core|nero|clean)"
        ;;
esac
