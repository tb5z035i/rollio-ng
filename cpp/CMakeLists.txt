cmake_minimum_required(VERSION 3.22)
project(rollio-cpp LANGUAGES CXX)

message(STATUS "Camera drivers moved to cameras/. Configuring legacy wrapper from cpp/.")
add_subdirectory("../cameras" "${CMAKE_BINARY_DIR}/cameras")
