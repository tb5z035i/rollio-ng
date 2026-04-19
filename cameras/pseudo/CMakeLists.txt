add_executable(rollio-device-pseudo-camera src/main.cpp)
target_link_libraries(rollio-device-pseudo-camera PRIVATE rollio-common)

if(ROLLIO_BUILD_CPP_TESTS)
    add_executable(rollio-device-pseudo-camera-tests tests/runtime.cpp)
    target_link_libraries(rollio-device-pseudo-camera-tests PRIVATE rollio-common)
    target_compile_definitions(
        rollio-device-pseudo-camera-tests
        PRIVATE
        ROLLIO_DEVICE_PSEUDO_CAMERA_BIN="$<TARGET_FILE:rollio-device-pseudo-camera>"
    )
    add_test(NAME rollio-device-pseudo-camera-tests COMMAND rollio-device-pseudo-camera-tests)
endif()
