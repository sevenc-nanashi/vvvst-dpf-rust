cmake_minimum_required(VERSION 3.24)

add_subdirectory(deps/corrosion)
add_subdirectory(deps/dpf)

project(vvvst)

if(CMAKE_BUILD_TYPE STREQUAL "Debug")
  add_definitions(-DDEBUG)
  set(VVVST_NAME "vvvst_debug")
  set(RUST_PROFILE "dev")
elseif(CMAKE_BUILD_TYPE STREQUAL "Release")
  set(VVVST_NAME "vvvst")
  set(RUST_PROFILE "release")
elseif(NOT CMAKE_BUILD_TYPE)
  message(FATAL_ERROR "Build type not set")
else()
  message(FATAL_ERROR "Unknown build type: ${CMAKE_BUILD_TYPE}")
endif()

corrosion_import_crate(
  MANIFEST_PATH ${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml
  PROFILE ${RUST_PROFILE}
  CRATES vvvst-dpf-rust)

# MSVC専用：utf-8を使う
if(MSVC)
  add_compile_options("/utf-8")
endif()

dpf_add_plugin(
  ${VVVST_NAME}
  TARGETS vst3 au
  UI_TYPE external
  FILES_DSP src/plugin.cpp
  FILES_UI src/ui.cpp)

if(APPLE)
  enable_language(OBJC)
  target_link_libraries(${VVVST_NAME} PRIVATE "-framework CoreFoundation")
  target_link_libraries(${VVVST_NAME} PRIVATE "-framework CoreGraphics")
  target_link_libraries(${VVVST_NAME} PRIVATE "-framework AppKit")
  target_link_libraries(${VVVST_NAME} PRIVATE "-framework QuartzCore")
  set_target_properties(${VVVST_NAME} PROPERTIES LINK_FLAGS "-Wl,-F/Library/Frameworks")
endif()

target_link_libraries(${VVVST_NAME} PRIVATE vvvst_dpf_rust)
target_include_directories(${VVVST_NAME} PUBLIC "src" "deps")
target_compile_features(${VVVST_NAME} PUBLIC cxx_std_17)
