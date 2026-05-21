# cameras/camera-cora/CMakeLists.txt
#
# Single Rollio camera device executable bridging Cora camera topics into the
# Rollio iceoryx2 bus as standard CameraFrameHeader frames. The driver is
# built on the Cora SDK (framework::ChannelReader<T, PubSubType>) instead of
# a hand written Fast-DDS subscriber + CDR parser. The three physical mount
# points (head / lefthand / righthand) are selected at run time via the
# BinaryDeviceConfig.id field (cora-head / cora-lefthand / cora-righthand).
#
# When a channel's DDS topic name is empty (e.g. in unit tests without a
# DDS stack) the worker falls back to the internal mock generator so that
# the iceoryx2 publish path and encoder passthrough integration remain
# testable offline.
#
# See signal_other/analysis/rollio-device/coracam-single-binary-multi-process-setup实施方案.zh.md.

find_package(Threads REQUIRED)
find_package(cora REQUIRED CONFIG)

add_library(camera-cora-runtime STATIC
    src/device_main.cpp
    src/channel_worker.cpp
    src/h264_annexb.cpp
    src/cora_types.cpp
    src/cora_mapping.cpp
)
target_include_directories(camera-cora-runtime PUBLIC
    "${CMAKE_CURRENT_SOURCE_DIR}/src"
)
# Cora SDK targets are declared with cxx_std_20; consumers must follow.
target_compile_features(camera-cora-runtime PUBLIC cxx_std_20)
target_link_libraries(camera-cora-runtime PUBLIC
    rollio-common
    Threads::Threads
    cora::cora_framework
)

add_executable(rollio-device-camera-cora apps/coracam.cpp)
target_link_libraries(rollio-device-camera-cora PRIVATE camera-cora-runtime)
set_target_properties(rollio-device-camera-cora PROPERTIES
    BUILD_RPATH "${ROLLIO_CORA_SDK_ROOT}/lib;/opt/cora/lib"
    INSTALL_RPATH "/opt/cora/lib"
)

if(ROLLIO_BUILD_CPP_TESTS)
    add_executable(camera-cora-tests tests/runtime.cpp)
    target_link_libraries(camera-cora-tests PRIVATE camera-cora-runtime)
    add_dependencies(camera-cora-tests rollio-device-camera-cora)
    target_compile_definitions(
        camera-cora-tests
        PRIVATE
        ROLLIO_DEVICE_CORACAM_BIN="$<TARGET_FILE:rollio-device-camera-cora>"
    )
    # Set ROLLIO_CORACAM_NO_DDS=1 so the runtime test uses the internal mock
    # generator path rather than attempting a live Cora SDK connection.
    add_test(NAME camera-cora-tests COMMAND camera-cora-tests)
    set_tests_properties(camera-cora-tests PROPERTIES
        ENVIRONMENT "ROLLIO_CORACAM_NO_DDS=1"
    )
endif()
