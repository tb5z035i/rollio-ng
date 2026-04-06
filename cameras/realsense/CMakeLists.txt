find_package(realsense2 QUIET)

add_executable(rollio-camera-realsense src/main.cpp)
target_link_libraries(rollio-camera-realsense PRIVATE rollio-common)

if(TARGET realsense2::realsense2)
    target_link_libraries(rollio-camera-realsense PRIVATE realsense2::realsense2)
    target_compile_definitions(rollio-camera-realsense PRIVATE ROLLIO_HAVE_REALSENSE=1)
elseif(realsense2_FOUND)
    target_link_libraries(rollio-camera-realsense PRIVATE ${realsense2_LIBRARY})
    target_compile_definitions(rollio-camera-realsense PRIVATE ROLLIO_HAVE_REALSENSE=1)
else()
    target_compile_definitions(rollio-camera-realsense PRIVATE ROLLIO_HAVE_REALSENSE=0)
endif()

if(ROLLIO_BUILD_CPP_TESTS)
    add_executable(rollio-camera-realsense-tests tests/runtime.cpp)
    target_link_libraries(rollio-camera-realsense-tests PRIVATE rollio-common)
    target_compile_definitions(
        rollio-camera-realsense-tests
        PRIVATE
        ROLLIO_CAMERA_REALSENSE_BIN="$<TARGET_FILE:rollio-camera-realsense>"
    )
    add_test(NAME rollio-camera-realsense-tests COMMAND rollio-camera-realsense-tests)
endif()
