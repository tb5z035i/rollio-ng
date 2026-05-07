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
add_subdirectory(
    "../third_party/librealsense"
    "${CMAKE_BINARY_DIR}/third_party/librealsense"
    EXCLUDE_FROM_ALL
)

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

add_subdirectory(pseudo)
add_subdirectory(realsense)

# ---------------------------------------------------------------------------
# Fast-DDS + dependencies for the UMI bridge device.
# ---------------------------------------------------------------------------
# rollio-device-umi subscribes to FastDDS topics on the cora side and
# republishes onto iceoryx2. We build Fast-DDS (3.6.x), Fast-CDR (bundled
# under Fast-DDS/thirdparty), and foonathan_memory (separate submodule)
# entirely in-tree so the resulting binary has no FastDDS runtime
# dependency. The build is opt-out via ROLLIO_BUILD_UMI_BRIDGE so anyone
# who hasn't initialized the Fast-DDS submodule (e.g. a minimal dev
# checkout without the bridge) can still build the rest.
option(ROLLIO_BUILD_UMI_BRIDGE
       "Build rollio-device-umi (requires third_party/Fast-DDS, Fast-CDR, foonathan_memory submodules)"
       ON)

if(ROLLIO_BUILD_UMI_BRIDGE
   AND EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/../third_party/Fast-DDS/CMakeLists.txt"
   AND EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/../third_party/foonathan_memory/CMakeLists.txt")

    # foonathan_memory must be configured before Fast-DDS calls
    # find_package(foonathan_memory REQUIRED). Disabling its tools and
    # examples mirrors the librealsense flag-flip pattern above.
    set(FOONATHAN_MEMORY_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
    set(FOONATHAN_MEMORY_BUILD_TESTS OFF CACHE BOOL "" FORCE)
    set(FOONATHAN_MEMORY_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
    add_subdirectory(
        "../third_party/foonathan_memory"
        "${CMAKE_BINARY_DIR}/third_party/foonathan_memory"
        EXCLUDE_FROM_ALL
    )

    # Fast-DDS uses find_package(foonathan_memory REQUIRED) which expects a
    # CMake config package on disk. Synthesise one by exporting the in-tree
    # target's properties: write a tiny wrapper config file into the build
    # tree and prepend it to CMAKE_PREFIX_PATH so Fast-DDS's find_package
    # resolves to our submodule build instead of looking for an installed
    # libfoonathan_memory-dev package.
    set(_foonathan_cfg_dir "${CMAKE_BINARY_DIR}/foonathan_memory_stub")
    file(MAKE_DIRECTORY "${_foonathan_cfg_dir}")
    file(WRITE "${_foonathan_cfg_dir}/foonathan_memoryConfig.cmake"
"# Generated stub: redirects find_package(foonathan_memory) to the in-tree
# add_subdirectory target so Fast-DDS resolves it without an installed
# system package.
if(NOT TARGET foonathan_memory)
    message(FATAL_ERROR \"foonathan_memory target missing — add_subdirectory failed\")
endif()
if(NOT TARGET foonathan_memory::foonathan_memory)
    add_library(foonathan_memory::foonathan_memory ALIAS foonathan_memory)
endif()
set(foonathan_memory_FOUND TRUE)
set(FOONATHAN_MEMORY_INCLUDE_DIR \"${CMAKE_CURRENT_LIST_DIR}\")
")
    file(WRITE "${_foonathan_cfg_dir}/foonathan_memoryConfigVersion.cmake"
"set(PACKAGE_VERSION \"0.7.4\")
set(PACKAGE_VERSION_COMPATIBLE TRUE)
set(PACKAGE_VERSION_EXACT FALSE)
")
    list(PREPEND CMAKE_PREFIX_PATH "${_foonathan_cfg_dir}")

    # Fast-CDR comes bundled under third_party/Fast-DDS/thirdparty/fastcdr.
    # Add it directly so the Fast-DDS find_package(fastcdr 2 REQUIRED) finds
    # our in-tree target. Disable its docs/tests/examples like the rest.
    set(BUILD_FASTCDR_DOCS OFF CACHE BOOL "" FORCE)
    set(EPROSIMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
    set(BUILD_TESTING OFF CACHE BOOL "" FORCE)
    add_subdirectory(
        "../third_party/Fast-DDS/thirdparty/fastcdr"
        "${CMAKE_BINARY_DIR}/third_party/fastcdr"
        EXCLUDE_FROM_ALL
    )

    # Disable every Fast-DDS optional feature so the static link stays
    # minimal and dpkg-shlibdeps only sees libstdc++ / libpthread / libc /
    # libdl / libm at packaging time.
    set(SECURITY OFF CACHE BOOL "" FORCE)             # no OpenSSL link
    set(FASTDDS_STATISTICS OFF CACHE BOOL "" FORCE)   # no perf monitoring
    set(COMPILE_EXAMPLES OFF CACHE BOOL "" FORCE)
    set(SQLITE3_SUPPORT OFF CACHE BOOL "" FORCE)
    set(BUILD_DOCUMENTATION OFF CACHE BOOL "" FORCE)
    set(NO_TLS ON CACHE BOOL "" FORCE)
    add_subdirectory(
        "../third_party/Fast-DDS"
        "${CMAKE_BINARY_DIR}/third_party/Fast-DDS"
        EXCLUDE_FROM_ALL
    )

    # The bridge binary lives outside cameras/ but is added here because this
    # is currently the only top-level C++ entry point. Future top-level
    # CMakeLists.txt could move this to a dedicated devices/ subdirectory.
    add_subdirectory(
        "../devices/umi"
        "${CMAKE_BINARY_DIR}/devices/umi"
    )
endif()
