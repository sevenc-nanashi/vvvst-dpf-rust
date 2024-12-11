
#include <choc/platform/choc_DynamicLibrary.h>
#include <choc/platform/choc_Platform.h>
#include <optional>
#include <shared_mutex>
#include <whereami.h>

namespace Rust {
std::optional<choc::file::DynamicLibrary> lib;
std::shared_mutex libMutex;

choc::file::DynamicLibrary *loadRustDll() {
  std::shared_lock lock(libMutex);
  if (lib.has_value()) {
    return &lib.value();
  }
  lock.unlock();

  std::unique_lock ulock(libMutex);

  auto modulePathSize = wai_getModulePath(nullptr, 0, nullptr);
  if (modulePathSize == -1) {
    throw std::runtime_error("Failed to get module path size");
  }
  std::string modulePath(modulePathSize, '\0');
  int moduleDirSize;
  if (wai_getModulePath(modulePath.data(), modulePath.capacity(),
                        &moduleDirSize) == -1) {
    throw std::runtime_error("Failed to get module path");
  }

  auto moduleDir = modulePath.substr(0, moduleDirSize);

  auto libPath = moduleDir +
#if defined CHOC_WINDOWS
                 "/vvvst_impl.dll";
#elif defined CHOC_OSX
                 "/libvvvst_impl.dylib";
#else
                 "/libvvvst_impl.so";
#endif
  auto localLib = choc::file::DynamicLibrary(libPath);
  if (localLib.handle == nullptr) {
    throw std::runtime_error("Failed to load Rust library");
  }
  lib = std::move(localLib);
  return &lib.value();
}
} // namespace Rust
