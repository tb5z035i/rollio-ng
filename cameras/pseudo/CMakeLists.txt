add_executable(rollio-camera-pseudo src/main.cpp)
target_link_libraries(rollio-camera-pseudo PRIVATE rollio-common)

if(ROLLIO_BUILD_CPP_TESTS)
    add_executable(rollio-camera-pseudo-tests tests/runtime.cpp)
    target_link_libraries(rollio-camera-pseudo-tests PRIVATE rollio-common)
    target_compile_definitions(
        rollio-camera-pseudo-tests
        PRIVATE
        ROLLIO_CAMERA_PSEUDO_BIN="$<TARGET_FILE:rollio-camera-pseudo>"
    )
    add_test(NAME rollio-camera-pseudo-tests COMMAND rollio-camera-pseudo-tests)
endif()
