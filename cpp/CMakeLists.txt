cmake_minimum_required(VERSION 3.16)
project(rollio-cpp LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Shared header library (header-only)
add_library(rollio-common INTERFACE)
target_include_directories(rollio-common INTERFACE common/include)

add_subdirectory(pseudo-camera)
