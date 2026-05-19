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
#   CORA_SDK_ROOT       extracted Cora SDK root used for packaging the
#                       arm64 coracam runtime (default: prebuild/.../opt/cora)

set -Eeuo pipefail
shopt -s inherit_errexit 2>/dev/null || true

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

DEB_VERSION="${DEB_VERSION:-1.0.0-1}"
DEB_ARCH="${DEB_ARCH:-$(dpkg --print-architecture 2>/dev/null || echo amd64)}"
DEB_DIST="${DEB_DIST:-dist}"
STAGING="${STAGING:-.deb-staging}"
TARGET_DIR="${TARGET_DIR:-target/release}"
CAMERAS_BUILD_DIR="${CAMERAS_BUILD_DIR:-cameras/build}"
CORA_SDK_ROOT="${CORA_SDK_ROOT:-prebuild/cora-sdk_1.2.0_20260517124657_linux_aarch64/opt/cora}"

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
    rollio-web-gateway
    rollio-teleop-router
    rollio-episode-lerobot
    rollio-episode-mcap
    rollio-storage-local
    rollio-storage-local-lerobot
    rollio-monitor
    rollio-device-pseudo
    rollio-device-airbot-play
    rollio-device-v4l2
    rollio-device-imu-cora
    rollio-device-tactile-cora
    rollio-device-gripper-cora
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
    "rollio-devices-coracam/rollio-device-coracam"
)

# On arm64, include the Horizon X5 VPU encoder binary.
if [[ "$DEB_ARCH" == "arm64" ]]; then
    CORE_BINS+=( rollio-encoder-x5 )
    SHLIBDEPS_EXCLUDE_BINS+=( rollio-encoder-x5 )
fi

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
        WHEEL_OUT_FLAG=--out-dir
    elif python3 -c 'import build' >/dev/null 2>&1; then
        WHEEL_BUILDER=(python3 -m build --wheel)
        WHEEL_OUT_FLAG=--outdir
    else
        die "need uv (preferred) or python3-build for the Nero wheel; try \`pipx install uv\`"
    fi
}

assert_built() {
    for b in "${CORE_BINS[@]}"; do
        [[ -x "$TARGET_DIR/$b" ]] || die "missing $TARGET_DIR/$b -- run \`make build\` (or \`make package-all\`) first"
    done
    for entry in "${CAMERA_BINS[@]}"; do
        if [[ -x "$CAMERAS_BUILD_DIR/$entry" ]]; then
            :
        elif [[ "${ROLLIO_SKIP_CAMERAS:-}" == "1" ]]; then
            warn "skipping $CAMERAS_BUILD_DIR/$entry (ROLLIO_SKIP_CAMERAS=1)"
        else
            die "missing $CAMERAS_BUILD_DIR/$entry -- run \`make cpp-build\` (or \`make build\`) first"
        fi
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
    # Cross-build sanity: when packing arm64 the vendor tree MUST contain the
    # arm64 sharp binding, not the host x86_64 one. npm picked whatever the
    # build runner's --cpu/--os flags asked for (see `make ui-build-arm64`),
    # so a mismatch here means the wrong tree got packaged. Loud failure beats
    # a deb that segfaults on the operator's box.
    case "$DEB_ARCH" in
        arm64)
            local sharp_native_glob="ui/terminal/.deb-vendor/node_modules/@img/sharp-linux-arm64"
            local sharp_libvips_glob="ui/terminal/.deb-vendor/node_modules/@img/sharp-libvips-linux-arm64"
            ;;
        amd64)
            local sharp_native_glob="ui/terminal/.deb-vendor/node_modules/@img/sharp-linux-x64"
            local sharp_libvips_glob="ui/terminal/.deb-vendor/node_modules/@img/sharp-libvips-linux-x64"
            ;;
        *)
            log "DEB_ARCH=$DEB_ARCH: skipping @img/sharp-* arch sanity check"
            return 0
            ;;
    esac
    [[ -d "$sharp_native_glob" ]] \
        || die "missing $sharp_native_glob for DEB_ARCH=$DEB_ARCH -- run \`make ui-build-arm64\` (or set --cpu/--os on \`npm ci\`)"
    [[ -d "$sharp_libvips_glob" ]] \
        || die "missing $sharp_libvips_glob for DEB_ARCH=$DEB_ARCH -- run \`make ui-build-arm64\` (or set --cpu/--os on \`npm ci\`)"
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

    # Cross-arch shlibdeps: when DEB_ARCH does not match the host arch,
    # dpkg-shlibdeps relies on Debian multiarch (`:arm64` packages installed
    # alongside the host's :amd64 set, registered in /var/lib/dpkg) to map
    # foreign-arch sonames to package names. `make deps TARGET_ARCH=arm64` enables
    # that. Even with multiarch fully wired some private/internal libs (e.g.
    # ones loaded by FFmpeg's filters with no ldconfig record) cannot be
    # mapped; --ignore-missing-info downgrades those from fatal to warnings,
    # mirroring how the amd64 path tolerates `rollio-encoder` via the
    # SHLIBDEPS_EXCLUDE_BINS list. Hermetic CI can opt into a chroot run
    # instead by setting ROLLIO_DEB_SHLIBDEPS_CHROOT to the path of a
    # pre-bootstrapped target-arch rootfs (qemu-user-static + binfmt handles
    # the syscall translation transparently).
    local host_arch
    host_arch="$(dpkg --print-architecture 2>/dev/null || echo "$DEB_ARCH")"
    local extra_args=()
    local private_lib_dirs=()
    if [[ -d "$root_abs/opt/cora/lib" ]]; then
        private_lib_dirs+=("$root_abs/opt/cora/lib")
        extra_args+=("-l$root_abs/opt/cora/lib")
    fi
    if [[ "$DEB_ARCH" != "$host_arch" ]]; then
        extra_args+=(--ignore-missing-info)
        log "Cross-arch shlibdeps: DEB_ARCH=$DEB_ARCH host=$host_arch (multiarch :$DEB_ARCH packages required)"
    fi

    if [[ -n "${ROLLIO_DEB_SHLIBDEPS_CHROOT:-}" ]]; then
        run_shlibdeps_in_chroot \
            "$ROLLIO_DEB_SHLIBDEPS_CHROOT" "$ctldir" "$subst_abs" \
            "${private_lib_dirs[@]}" -- "${elfs[@]}"
        return $?
    fi

    ( cd "$ctldir" && dpkg-shlibdeps "${extra_args[@]}" -T"$subst_abs" -pshlibs "${elfs[@]}" )
}

# Hermetic alternative for run_shlibdeps. Bind-mounts the staging tree into
# a target-arch chroot (typically created with `debootstrap --arch=arm64`
# pointed at /var/lib/rollio/arm64-rootfs or similar) and runs dpkg-shlibdeps
# inside it via qemu-user-static (auto-handled by binfmt_misc when the
# `qemu-user-static` apt package is installed).
#
# Used by CI to avoid relying on the host's multiarch admin DB; not normally
# needed for local dev when `make deps TARGET_ARCH=arm64` was run.
run_shlibdeps_in_chroot() {
    local chroot_root="$1" ctldir="$2" subst_abs="$3"
    shift 3
    local private_lib_dirs=()
    while [[ $# -gt 0 && "$1" != "--" ]]; do
        private_lib_dirs+=("$1")
        shift
    done
    [[ $# -gt 0 && "$1" == "--" ]] && shift
    local elfs=("$@")
    [[ -d "$chroot_root" ]] \
        || die "ROLLIO_DEB_SHLIBDEPS_CHROOT=$chroot_root is not a directory"
    command -v sudo >/dev/null 2>&1 \
        || die "ROLLIO_DEB_SHLIBDEPS_CHROOT requires sudo to bind-mount and chroot"

    log "Running dpkg-shlibdeps inside chroot $chroot_root"
    # Stage the synthesized debian/control + the ELFs under a temp dir
    # *inside* the chroot so the in-chroot dpkg-shlibdeps can read them.
    local rel_workdir=".rollio-shlibdeps-$$"
    local in_chroot_workdir="$chroot_root/$rel_workdir"
    sudo mkdir -p "$in_chroot_workdir"
    sudo cp -a "$ctldir/." "$in_chroot_workdir/"
    local i=0
    local elf_args=()
    for elf in "${elfs[@]}"; do
        local rel="elf-$i-$(basename "$elf")"
        sudo cp "$elf" "$in_chroot_workdir/$rel"
        elf_args+=("/$rel_workdir/$rel")
        i=$((i+1))
    done
    local private_args=()
    i=0
    for dir in "${private_lib_dirs[@]}"; do
        [[ -d "$dir" ]] || continue
        local rel="private-lib-$i"
        sudo mkdir -p "$in_chroot_workdir/$rel"
        sudo cp -a "$dir/." "$in_chroot_workdir/$rel/"
        private_args+=("-l/$rel_workdir/$rel")
        i=$((i+1))
    done
    sudo cp "$subst_abs" "$in_chroot_workdir/substvars" 2>/dev/null || true

    local shlibdeps_args=(--ignore-missing-info "${private_args[@]}" -Tsubstvars -pshlibs "${elf_args[@]}")
    sudo chroot "$chroot_root" \
        /bin/sh -c "cd /$rel_workdir && dpkg-shlibdeps $(printf ' %q' "${shlibdeps_args[@]}")"
    local rc=$?
    if [[ $rc -eq 0 && -f "$in_chroot_workdir/substvars" ]]; then
        sudo cp "$in_chroot_workdir/substvars" "$subst_abs"
    fi
    sudo rm -rf "$in_chroot_workdir"
    return $rc
}

stage_cora_sdk_runtime() {
    # The Cora SDK archive is committed as a prebuild tarball and extracted by
    # Makefile's prepare-cora-sdk target. Package only the runtime closure that
    # coracam needs, not the SDK headers, CMake metadata, Python bindings, or
    # debug object trees.
    local root="$1"
    [[ "$DEB_ARCH" == "arm64" ]] || return 0
    [[ -x "$root/usr/bin/rollio-device-coracam" ]] || return 0

    local src="$CORA_SDK_ROOT"
    [[ -d "$src/lib" ]] || die "missing Cora SDK lib dir: $src/lib -- run \`make prepare-cora-sdk\`"

    log "Staging Cora SDK runtime from $src -> $root/opt/cora"
    install -d "$root/opt/cora/lib"
    cp -a "$src/lib"/*.so* "$root/opt/cora/lib/"
    if [[ -d "$src/lib/cora_framework" ]]; then
        cp -a "$src/lib/cora_framework" "$root/opt/cora/lib/"
    fi
    if [[ -d "$src/share" ]]; then
        cp -a "$src/share" "$root/opt/cora/"
    fi
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
        if [[ -f "$CAMERAS_BUILD_DIR/$entry" ]]; then
            install -m755 "$CAMERAS_BUILD_DIR/$entry" "$CORE_STAGING/usr/bin/"
        else
            warn "skipping $entry (not built; ROLLIO_SKIP_CAMERAS=1)"
        fi
    done
    stage_cora_sdk_runtime "$CORE_STAGING"
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
    rm -f "$DEB_DIST"/rollio_device_nero-*.whl
    log "Building Nero wheel via ${WHEEL_BUILDER[*]}"
    # `uv build` and `python -m build` both accept a project directory,
    # but their output-dir flag spelling differs.
    "${WHEEL_BUILDER[@]}" "$WHEEL_OUT_FLAG" "$DEB_DIST" robots/nero >&2 \
        || die "Nero wheel build failed"
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
        out="$(build_core)" || exit 1
        log "Done: $out"
        ;;
    nero)
        outs="$(build_nero)" || exit 1
        log "Done:"
        printf '  %s\n' $outs >&2
        ;;
    all)
        c="$(build_core)" || exit 1
        n="$(build_nero)" || exit 1
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
