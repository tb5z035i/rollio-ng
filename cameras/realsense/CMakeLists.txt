find_package(realsense2 QUIET)

add_executable(rollio-device-realsense src/main.cpp)
target_link_libraries(rollio-device-realsense PRIVATE rollio-common)

if(TARGET realsense2::realsense2)
    target_link_libraries(rollio-device-realsense PRIVATE realsense2::realsense2)
    target_compile_definitions(rollio-device-realsense PRIVATE ROLLIO_HAVE_REALSENSE=1)
elseif(realsense2_FOUND)
    target_link_libraries(rollio-device-realsense PRIVATE ${realsense2_LIBRARY})
    target_compile_definitions(rollio-device-realsense PRIVATE ROLLIO_HAVE_REALSENSE=1)
else()
    target_compile_definitions(rollio-device-realsense PRIVATE ROLLIO_HAVE_REALSENSE=0)
endif()

if(ROLLIO_BUILD_CPP_TESTS)
    add_executable(rollio-device-realsense-tests tests/runtime.cpp)
    target_link_libraries(rollio-device-realsense-tests PRIVATE rollio-common)
    target_compile_definitions(
        rollio-device-realsense-tests
        PRIVATE
        ROLLIO_DEVICE_REALSENSE_BIN="$<TARGET_FILE:rollio-device-realsense>"
    )
    add_test(NAME rollio-device-realsense-tests COMMAND rollio-device-realsense-tests)
endif()
