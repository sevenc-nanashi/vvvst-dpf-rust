cmake_minimum_required(VERSION 3.24)

add_subdirectory(deps/corrosion)
add_subdirectory(deps/dpf)

project(vvvst)
if(CMAKE_BUILD_TYPE STREQUAL "Debug")
  add_definitions(-DDEBUG)
elseif(CMAKE_BUILD_TYPE STREQUAL "Release")

elseif(NOT CMAKE_BUILD_TYPE)
  message(FATAL_ERROR "Build type not set")
else()
  message(FATAL_ERROR "Unknown build type: ${CMAKE_BUILD_TYPE}")
endif()

corrosion_import_crate(MANIFEST_PATH ${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml
                       CRATES vvvst-dpf-rust)

# MSVC専用：utf-8を使う
if(MSVC)
  add_compile_options("/utf-8")
endif()

dpf_add_plugin(
  vvvst
  TARGETS vst3
  UI_TYPE external
  FILES_DSP src/plugin.cpp
  FILES_UI src/ui.cpp)

target_link_libraries(vvvst PRIVATE vvvst_dpf_rust)
target_include_directories(vvvst PUBLIC "src" "deps")
target_compile_features(vvvst PUBLIC cxx_std_17)
