# cameras/rollio-devices-coracam/CMakeLists.txt
#
# Three Rollio camera devices that bridge Cora / Fast-DDS camera topics into
# the Rollio iceoryx2 bus as standard CameraFrameHeader frames samples.
#
# Fast-DDS and foonathan memory are vendored under third_party/ and built
# in cameras/CMakeLists.txt when ROLLIO_BUILD_CORACAM=ON.  The channel
# workers use a real Fast-DDS DataReader per channel.  When a channel's
# DDS topic name is empty (e.g. in unit tests without a DDS stack) the
# worker falls back to the internal mock generator so that the iceoryx2
# publish path and encoder passthrough integration remain testable offline.
#
# See signal_other/analysis/rollio-device/cora-topic-to-rollio-bus-device方案.zh.md
# and signal_other/analysis/rollio-device/coracam-h264-annexb实施方案.zh.md.

find_package(Threads REQUIRED)

add_library(rollio-devices-coracam-runtime STATIC
    src/device_main.cpp
    src/channel_worker.cpp
    src/h264_annexb.cpp
    src/cora_types.cpp
    src/cora_subscriber.cpp
    src/cora_mapping.cpp
)
target_include_directories(rollio-devices-coracam-runtime PUBLIC
    "${CMAKE_CURRENT_SOURCE_DIR}/src"
)
target_link_libraries(rollio-devices-coracam-runtime PUBLIC
    rollio-common
    Threads::Threads
    fastdds
    fastcdr
)
# Expose the Fast-DDS and Fast-CDR headers (they are PUBLIC deps of fastdds
# but the include path is not always automatically propagated by the target
# when built from source with EXCLUDE_FROM_ALL).
target_include_directories(rollio-devices-coracam-runtime PUBLIC
    "${CMAKE_BINARY_DIR}/third_party/Fast-DDS/thirdparty/fastcdr/include"
    "${CMAKE_SOURCE_DIR}/../third_party/Fast-DDS/include"
    "${CMAKE_SOURCE_DIR}/../third_party/Fast-DDS/thirdparty/fastcdr/include"
)

foreach(_device head lefthand righthand)
    set(_target rollio-device-coracam-${_device})
    add_executable(${_target} apps/coracam_${_device}.cpp)
    target_link_libraries(${_target} PRIVATE rollio-devices-coracam-runtime)
endforeach()

if(ROLLIO_BUILD_CPP_TESTS)
    add_executable(rollio-devices-coracam-tests tests/runtime.cpp)
    target_link_libraries(rollio-devices-coracam-tests PRIVATE rollio-devices-coracam-runtime)
    add_dependencies(rollio-devices-coracam-tests rollio-device-coracam-head)
    target_compile_definitions(
        rollio-devices-coracam-tests
        PRIVATE
        ROLLIO_DEVICE_CORACAM_HEAD_BIN="$<TARGET_FILE:rollio-device-coracam-head>"
    )
    # Set ROLLIO_CORACAM_NO_DDS=1 so the runtime test uses the internal mock
    # generator path rather than attempting a live Fast-DDS connection.
    add_test(NAME rollio-devices-coracam-tests COMMAND rollio-devices-coracam-tests)
    set_tests_properties(rollio-devices-coracam-tests PROPERTIES
        ENVIRONMENT "ROLLIO_CORACAM_NO_DDS=1"
    )
endif()
