cmake_minimum_required(VERSION 3.24)

# 普通にadd_subdirectoryすると、なぜかvcpkgで無限ループが発生するので、
# 直接DPF-plugin.cmakeをincludeする
set(DPF_ROOT_DIR "${CMAKE_CURRENT_SOURCE_DIR}/deps/dpf")
include(./deps/dpf/cmake/DPF-plugin.cmake)

project(vvvst)
add_library(rust STATIC IMPORTED)
if(CMAKE_BUILD_TYPE STREQUAL "Debug")
  add_definitions(-DDEBUG)
  set_target_properties(
    rust
    PROPERTIES IMPORTED_LOCATION
               "${CMAKE_CURRENT_SOURCE_DIR}/target/debug/vvvst_dpf_rust.lib")
elseif(CMAKE_BUILD_TYPE STREQUAL "Release")

elseif(NOT CMAKE_BUILD_TYPE)
  message(FATAL_ERROR "Build type not set")
else()
  message(FATAL_ERROR "Unknown build type: ${CMAKE_BUILD_TYPE}")
endif()

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

# メモ：cargo rustc -- --print native-static-libs でリンクするライブラリを調べる
if(MSVC)
  link_directories(CMAKE_WINDOWS_KITS_10_DIR)
  target_link_libraries(vvvst PRIVATE kernel32.lib advapi32.lib ntdll.lib
                                      userenv.lib ws2_32.lib kernel32.lib)
  target_link_libraries(
    vvvst
    PRIVATE
      "$ENV{USERPROFILE}/.cargo/registry/src/index.crates.io-6f17d22bba15001f/windows_x86_64_msvc-0.52.0/lib/windows.0.52.0.lib"
  )
endif()

target_link_libraries(vvvst PRIVATE rust)
target_include_directories(vvvst PUBLIC "src" "deps")
target_compile_features(vvvst PUBLIC cxx_std_17)
