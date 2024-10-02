cmake_minimum_required(VERSION 3.24)

set(DPF_ROOT_DIR "${CMAKE_CURRENT_SOURCE_DIR}/deps/DPF")
include(./deps/DPF/cmake/DPF-plugin.cmake)

project(vvvst)
add_library(rust STATIC IMPORTED)
if(CMAKE_BUILD_TYPE STREQUAL "Debug")
  add_definitions(-DDEBUG)
  set_target_properties(rust PROPERTIES IMPORTED_LOCATION "${CMAKE_CURRENT_SOURCE_DIR}/target/debug/vvvst_dpf_rust.lib")
elseif(CMAKE_BUILD_TYPE STREQUAL "Release")
  set_target_properties(rust PROPERTIES IMPORTED_LOCATION "${CMAKE_CURRENT_SOURCE_DIR}/target/release/vvvst_dpf_rust.lib")
elseif(NOT CMAKE_BUILD_TYPE)
  message(FATAL_ERROR "Build type not set")
else()
  message(FATAL_ERROR "Unknown build type: ${CMAKE_BUILD_TYPE}")
endif()

dpf_add_plugin(
  vvvst
  TARGETS
    vst3
  UI_TYPE
    external
  FILES_DSP
    src/plugin.cpp
  FILES_UI
    src/ui.cpp)

# MSVC専用：
# - utf-8を使う
if(MSVC)
  add_compile_options("/utf-8")
endif()

target_link_libraries(vvvst PRIVATE rust)
target_include_directories(vvvst PUBLIC "src" "deps")
target_compile_features(vvvst PUBLIC cxx_std_17)
