# CMake cross-compile toolchain for `aarch64-unknown-linux-gnu`.
#
# Used by `make cpp-build TARGET_ARCH=arm64` to drive cmake against
# `cameras/` (and the librealsense + iceoryx2-cxx sub-builds it pulls in
# via add_subdirectory).
#
# Toolchain: clang as both compiler and linker frontend, lld as the
# actual linker. `--target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu`
# tells clang to emit aarch64 code and search the cross sysroot for
# headers/libs. binutils-aarch64-linux-gnu (pulled in by
# `crossbuild-essential-arm64`) supplies `ar`/`ranlib`/`strip` -- llvm
# equivalents (llvm-ar, llvm-strip) would also work but the gnu cross
# tools are already on the box.
#
# Prerequisites are installed by `make package-deps TARGET_ARCH=arm64`:
#   * clang, clang++, lld                -> /usr/bin/clang*, /usr/bin/lld
#   * crossbuild-essential-arm64         -> /usr/bin/aarch64-linux-gnu-{ar,strip,ranlib,ld,...}
#                                          (we only use the binutils side)
#   * dpkg multiarch + :arm64 dev libs   -> /usr/lib/aarch64-linux-gnu/**
#                                          /usr/aarch64-linux-gnu/include/**
#   * libstdc++-13-dev:arm64             -> arm64 libstdc++ headers + libs

set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)

# Use thin shell wrappers (cross-wrappers/aarch64-linux-gnu-clang*) as the
# CMAKE_<LANG>_COMPILER instead of bare `clang` / `clang++`. The wrappers
# bake `--target=aarch64-linux-gnu` into every invocation, including
# probe-style `clang -dumpmachine` calls that downstream CMake configs
# make outside the normal compile/link rules. librealsense's
# CMake/unix_config.cmake (third_party/librealsense/CMake/unix_config.cmake:21)
# is the canonical example -- it dumpmachine's the compiler and adds
# `-mssse3` unless the triple matches `aarch64-*`. Without the wrapper,
# `clang -dumpmachine` returns the host triple `x86_64-pc-linux-gnu` and
# librealsense pollutes CMAKE_C_FLAGS with x86 SIMD flags that clang
# (correctly cross-targeting aarch64) then rejects.
get_filename_component(_rollio_repo_root "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)
set(CMAKE_C_COMPILER   "${_rollio_repo_root}/cmake/cross-wrappers/aarch64-linux-gnu-clang")
set(CMAKE_CXX_COMPILER "${_rollio_repo_root}/cmake/cross-wrappers/aarch64-linux-gnu-clang++")

# `--target=aarch64-linux-gnu` is enough for clang to act as a cross
# compiler -- its gcc-detection auto-discovers the cross sysroot at
# /usr/aarch64-linux-gnu (from crossbuild-essential-arm64) AND the
# multiarch :arm64 dev libs at /usr/include/aarch64-linux-gnu (from
# `make deps TARGET_ARCH=arm64`). DO NOT pass --sysroot=/usr/aarch64-linux-gnu
# explicitly: the libc.so linker script in that prefix has hardcoded
# absolute paths to /usr/aarch64-linux-gnu/lib/libc.so.6 etc., and an
# explicit --sysroot makes lld interpret those as relative to the
# sysroot, doubling the prefix and 404'ing.
#
# CMAKE_<LANG>_FLAGS_INIT only seeds the cache entry the FIRST time a
# build tree is configured -- if the cmake driver (e.g. cmake-rs from
# turbojpeg-sys) already passed -DCMAKE_C_FLAGS=... on the command
# line, the cache entry is already set and _INIT is ignored. We FORCE
# an append instead: ${CMAKE_C_FLAGS} reads whatever the driver set
# (or empty if nothing), then we tack on our cross flags. The cmake
# driver's flags are preserved (e.g. -ffunction-sections -fPIC -w),
# our cross flags are guaranteed present, and clang's last-wins rule
# makes any duplicate --target harmless.
set(_rollio_cross_flags "--target=aarch64-linux-gnu")
set(CMAKE_C_FLAGS   "${CMAKE_C_FLAGS} ${_rollio_cross_flags}"   CACHE STRING "" FORCE)
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} ${_rollio_cross_flags}" CACHE STRING "" FORCE)
set(CMAKE_ASM_FLAGS "${CMAKE_ASM_FLAGS} ${_rollio_cross_flags}" CACHE STRING "" FORCE)

# Linker: clang again, with -fuse-ld=lld for cross-arch linking. lld
# natively supports aarch64 so we do not need aarch64-linux-gnu-ld here.
set(_rollio_cross_link "${_rollio_cross_flags} -fuse-ld=lld")
set(CMAKE_EXE_LINKER_FLAGS    "${CMAKE_EXE_LINKER_FLAGS} ${_rollio_cross_link}"
    CACHE STRING "" FORCE)
set(CMAKE_SHARED_LINKER_FLAGS "${CMAKE_SHARED_LINKER_FLAGS} ${_rollio_cross_link}"
    CACHE STRING "" FORCE)
set(CMAKE_MODULE_LINKER_FLAGS "${CMAKE_MODULE_LINKER_FLAGS} ${_rollio_cross_link}"
    CACHE STRING "" FORCE)

# Binutils tools come from binutils-aarch64-linux-gnu (cross GNU). lld
# does not provide ar / ranlib / strip / objcopy; the gnu cross binaries
# are already on PATH thanks to crossbuild-essential-arm64. CACHE FORCE
# so reconfigures stay deterministic.
set(CMAKE_AR     aarch64-linux-gnu-ar     CACHE FILEPATH "" FORCE)
set(CMAKE_RANLIB aarch64-linux-gnu-ranlib CACHE FILEPATH "" FORCE)
set(CMAKE_STRIP  aarch64-linux-gnu-strip  CACHE FILEPATH "" FORCE)

# iceoryx2's top-level CMakeLists builds its `iceoryx2-ffi-c` Rust crate
# in-tree via `cargo build`. By default it uses the host arch (no
# --target). When this toolchain file is loaded for a cross build,
# pass the Rust triple through so the resulting libiceoryx2_ffi_c.a
# is aarch64. See third_party/iceoryx2/CMakeLists.txt:177.
set(RUST_TARGET_TRIPLET "aarch64-unknown-linux-gnu"
    CACHE STRING "Cargo target triple for the in-tree iceoryx2-ffi-c sub-build")

set(CMAKE_FIND_ROOT_PATH /usr/lib/aarch64-linux-gnu /usr/aarch64-linux-gnu)
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
# LIBRARY = BOTH: find_library() re-roots HINTS/PATHS under the cross
# prefixes first, then falls back to the path as-given. ONLY would skip
# the as-given lookup, breaking find_library(NAMES x HINTS /usr/lib/aarch64-linux-gnu)
# because /usr/lib/aarch64-linux-gnu/usr/lib/aarch64-linux-gnu doesn't exist.
# That call site is exactly how pinocchio's vendored Findurdfdom.cmake
# (third_party/airbot-play-rust/ffi/cmake/Findurdfdom.cmake) locates the
# multiarch :arm64 .so files. The re-rooted lookup runs first, so the
# fallback only kicks in when the cross-prefixed path is empty -- it does
# not let a host x86_64 .so slip in ahead of an aarch64 one.
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY BOTH)
# INCLUDE = BOTH: many Debian dev packages are Multi-Arch: foreign
# (header-only, e.g. liburdfdom-headers-dev, libeigen3-dev) and ship
# headers under /usr/include/ rather than /usr/include/aarch64-linux-gnu/.
# Letting find_path() also search host paths picks those up. The compiler
# still uses arch-correct paths via clang's gcc-detection.
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE BOTH)
# PACKAGE = BOTH: same reasoning -- arch-agnostic CMake configs live in
# host paths (e.g. Eigen3 at /usr/share/eigen3/cmake/Eigen3Config.cmake).
# Boost, urdfdom, libusb etc. carry their CMake configs under
# /usr/lib/aarch64-linux-gnu/cmake/ which the cross root covers, so they
# resolve either way.
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE BOTH)

# Multiarch pkg-config. Ubuntu 24.04 ships no per-arch wrapper package;
# we use the plain `pkg-config` (pkgconf) executable + env vars to point
# it at the arm64 multiarch tree. PKG_CONFIG_LIBDIR replaces (not augments)
# the default search list so cmake does not silently see host x86_64 .pc
# files. PKG_CONFIG_SYSROOT_DIR=/ keeps absolute paths in -I/-L unchanged
# (the arm64 dev libs live at /usr/lib/aarch64-linux-gnu, not under a
# separate sysroot). NAMES preserves a fallback to a triple-prefixed
# wrapper if a future Ubuntu reintroduces one.
set(ENV{PKG_CONFIG_LIBDIR} "/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig")
set(ENV{PKG_CONFIG_SYSROOT_DIR} "/")
find_program(PKG_CONFIG_EXECUTABLE NAMES pkg-config aarch64-linux-gnu-pkg-config)
