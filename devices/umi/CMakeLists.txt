# rollio-device-umi — bridges cora's FastDDS camera + IMU topics onto
# rollio's iceoryx2 bus.
#
# Static link against Fast-DDS, Fast-CDR, foonathan_memory, and iceoryx2.
# Wire-format compatibility with cora's Fast-DDS 1.x publishers is handled
# by the hand-rolled PubSubTypes under src/generated/.

add_executable(
    rollio-device-umi
    src/main.cpp
    src/bridge.cpp
    src/config.cpp
    src/generated/cora_pubsubtypes.cpp
)

# rollio-common pulls in iceoryx2 + the rollio C++ headers.
target_link_libraries(rollio-device-umi PRIVATE rollio-common)

# Static Fast-DDS (and its bundled Fast-CDR + foonathan_memory). The targets
# are produced by the corresponding add_subdirectory calls in
# cameras/CMakeLists.txt and are guaranteed to exist when this directory is
# added (see the `ROLLIO_BUILD_UMI_BRIDGE` block there).
target_link_libraries(rollio-device-umi PRIVATE fastdds fastcdr foonathan_memory)

target_include_directories(
    rollio-device-umi
    PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/src"
    "${CMAKE_CURRENT_SOURCE_DIR}/src/generated"
)

target_compile_definitions(rollio-device-umi PRIVATE ROLLIO_HAVE_UMI_BRIDGE=1)

if(ROLLIO_BUILD_CPP_TESTS)
    add_executable(rollio-device-umi-tests tests/runtime.cpp)
    target_link_libraries(rollio-device-umi-tests PRIVATE rollio-common)
    target_compile_definitions(
        rollio-device-umi-tests
        PRIVATE
        ROLLIO_DEVICE_UMI_BIN="$<TARGET_FILE:rollio-device-umi>"
    )
    add_test(NAME rollio-device-umi-tests COMMAND rollio-device-umi-tests)
endif()
