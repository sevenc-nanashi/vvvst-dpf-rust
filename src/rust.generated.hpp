#pragma once
#ifdef WIN32
#define EXPORT __declspec(dllexport)
#else
#define EXPORT
#endif


#include <cstdarg>
#include <cstdint>
#include <cstdlib>
#include <ostream>
#include <new>

namespace Rust {

struct Plugin;

struct PluginUi;

extern "C" {

EXPORT Plugin *plugin_new();

EXPORT void plugin_drop(Plugin *plugin);

EXPORT PluginUi *plugin_ui_new(uintptr_t handle, const Plugin *plugin);

EXPORT uintptr_t plugin_ui_get_native_window_handle(const PluginUi *plugin_ui);

EXPORT void plugin_ui_set_size(const PluginUi *plugin_ui, uintptr_t width, uintptr_t height);

EXPORT void plugin_ui_drop(PluginUi *plugin_ui);

}  // extern "C"

}  // namespace Rust
