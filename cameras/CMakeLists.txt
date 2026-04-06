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
