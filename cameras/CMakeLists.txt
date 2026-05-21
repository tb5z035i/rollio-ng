cmake_minimum_required(VERSION 3.22)
project(rollio-cameras LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

option(ROLLIO_BUILD_CPP_TESTS "Build Rollio C++ test executables" ON)

set(BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(BUILD_CXX ON CACHE BOOL "" FORCE)
set(BUILD_TESTING OFF CACHE BOOL "" FORCE)
add_subdirectory("../third_party/iceoryx2" "${CMAKE_BINARY_DIR}/third_party/iceoryx2" EXCLUDE_FROM_ALL)

# Build librealsense2 in-tree as a static library so rollio-device-realsense
# bundles the realsense SDK and has no librealsense2.so runtime dependency.
# Operators need only libusb-1.0-0 / libudev1 from Ubuntu apt, both of which
# dpkg-shlibdeps can resolve normally (Intel's librealsense2 .so is *not* a
# Debian package). Reset BUILD_SHARED_LIBS to OFF so the realsense2 target's
# `add_library(... )` becomes STATIC (it has no explicit STATIC/SHARED kind);
# also disable every realsense bundled feature that isn't reachable from the
# `realsense2` target itself (graphical examples, GLSL/GL extensions, tools,
# Python bindings, FastDDS, libcurl-based update checks, embedded firmware
# downloads). See third_party/librealsense/CMake/lrs_options.cmake for the
# full option list. CACHE FORCE matches the iceoryx2 setup above so reruns
# of `make cpp-build` stay deterministic regardless of any leftover cache.
set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
set(BUILD_GRAPHICAL_EXAMPLES OFF CACHE BOOL "" FORCE)
set(BUILD_GLSL_EXTENSIONS OFF CACHE BOOL "" FORCE)
set(BUILD_TOOLS OFF CACHE BOOL "" FORCE)
set(BUILD_UNIT_TESTS OFF CACHE BOOL "" FORCE)
set(BUILD_PYTHON_BINDINGS OFF CACHE BOOL "" FORCE)
set(BUILD_WITH_DDS OFF CACHE BOOL "" FORCE)
set(BUILD_RS2_ALL OFF CACHE BOOL "" FORCE)
set(CHECK_FOR_UPDATES OFF CACHE BOOL "" FORCE)
set(IMPORT_DEPTH_CAM_FW OFF CACHE BOOL "" FORCE)
set(ENABLE_CCACHE OFF CACHE BOOL "" FORCE)
set(ROLLIO_CORA_SDK_ROOT
    "${CMAKE_CURRENT_LIST_DIR}/../prebuild/cora-sdk_1.2.0_20260517124657_linux_aarch64/opt/cora"
    CACHE PATH "Cora SDK root used by camera-cora")
# librealsense's CMake/unix_config.cmake unconditionally appends `-Werror`,
# `-pedantic`, `-Wno-deprecated-builtins`, etc. to CMAKE_CXX_FLAGS via plain
# `set()` (no PARENT_SCOPE), so the flags leak into every subdir added
# afterwards.  Save the pre-realsense flags so we can restore a clean baseline
# before adding Fast-DDS / foonathan_memory — those projects do not compile
# cleanly with `-Werror -Wpedantic` on clang ≥ 21.
set(_rollio_cxx_flags_before_realsense "${CMAKE_CXX_FLAGS}")
set(_rollio_c_flags_before_realsense   "${CMAKE_C_FLAGS}")
add_subdirectory(
    "../third_party/librealsense"
    "${CMAKE_BINARY_DIR}/third_party/librealsense"
    EXCLUDE_FROM_ALL
)
# Restore the baseline.  The realsense static lib was already configured with
# the polluted flags above; only subsequent subdirs see the restored flags.
set(CMAKE_CXX_FLAGS "${_rollio_cxx_flags_before_realsense}")
set(CMAKE_C_FLAGS   "${_rollio_c_flags_before_realsense}")

if(ROLLIO_BUILD_CPP_TESTS)
    enable_testing()
endif()

add_library(rollio-common INTERFACE)
target_include_directories(
    rollio-common
    INTERFACE
    "${CMAKE_CURRENT_SOURCE_DIR}/../cpp/common/include"
)
target_link_libraries(rollio-common INTERFACE iceoryx2-cxx::static-lib-cxx)

add_subdirectory(realsense)

# ── Coracam (Cora SDK H.264 Annex-B + raw camera bridge) ─────────────────
#
# Coracam now consumes the prebuilt Cora SDK shipped under
#   prebuild/cora-sdk_<version>_linux_aarch64/opt/cora
# instead of building Fast-DDS/foonathan_memory from third_party/.
# The SDK is auto-discovered: callers do not need to pass CMAKE_PREFIX_PATH.
#
# See signal_other/analysis/rollio-device/coracam-cora-sdk-migration.zh.md
option(ROLLIO_BUILD_CORACAM "Build Cora SDK Coracam bridge devices" OFF)
if(EXISTS "${ROLLIO_CORA_SDK_ROOT}/lib/cmake/cora/coraConfig.cmake")
    list(PREPEND CMAKE_PREFIX_PATH "${ROLLIO_CORA_SDK_ROOT}")
endif()

if(ROLLIO_BUILD_CORACAM)
    add_subdirectory(camera_cora)
endif()
