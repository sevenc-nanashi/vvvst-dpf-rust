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
  CRATES vvvst)

# Windows：utf-8を使う、C4661（DPF内のテンプレート周りの警告）を無視
if(MSVC)
  add_compile_options("/utf-8")
  add_compile_options("/wd4661")
endif()

dpf_add_plugin(
  ${VVVST_NAME}
  TARGETS vst3 au
  UI_TYPE external
  FILES_DSP src/plugin.cpp
  FILES_UI src/ui.cpp
  FILES_COMMON src/rust_bridge.cpp src/rust_bridge.generated.cpp
               deps/whereami/src/whereami.c)

if(APPLE)
  # Mac: arm64をサポート
  set(CMAKE_OSX_ARCHITECTURES "arm64")
  # もしx86_64もサポートする場合は以下のように設定 set(CMAKE_OSX_ARCHITECTURES "x86_64;arm64")
endif()

target_link_libraries(${VVVST_NAME} PRIVATE vvvst_impl)
target_include_directories(${VVVST_NAME} PUBLIC "src" "deps"
                                                "deps/whereami/src")
target_compile_features(${VVVST_NAME} PUBLIC cxx_std_20)

# https://qiita.com/canary_llama/items/d63f5007161cf4a2c4b3
function(copy_dlls_to_target TARGET_NAME)
  add_custom_command(
    TARGET ${TARGET_NAME}
    POST_BUILD
    COMMAND
      ${CMAKE_COMMAND} -E echo Copying DLLs to
      $<TARGET_FILE_DIR:${TARGET_NAME}>: $<TARGET_RUNTIME_DLLS:${TARGET_NAME}>
    COMMAND_EXPAND_LISTS)
  add_custom_command(
    TARGET ${TARGET_NAME}
    POST_BUILD
    COMMAND
      ${CMAKE_COMMAND} -E copy_if_different
      $<TARGET_RUNTIME_DLLS:${TARGET_NAME}> $<TARGET_FILE_DIR:${TARGET_NAME}>
    COMMAND_EXPAND_LISTS)
endfunction()

copy_dlls_to_target(${VVVST_NAME}-vst3)
if(TARGET ${VVVST_NAME}-au)
  copy_dlls_to_target(${VVVST_NAME}-au)
endif()
