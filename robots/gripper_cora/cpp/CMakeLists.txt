cmake_minimum_required(VERSION 3.22)
project(gripper_cora_shim CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

find_package(cora REQUIRED)

add_library(cora_bridge_shim STATIC
    src/bridge.cpp
    src/subscriber.cpp)

target_include_directories(cora_bridge_shim
    PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/include)

target_link_libraries(cora_bridge_shim
    PUBLIC cora::framework)

install(DIRECTORY include/ DESTINATION include)
install(TARGETS cora_bridge_shim ARCHIVE DESTINATION lib)
