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

struct Version {
  uint8_t major;
  uint8_t minor;
  uint8_t patch;
};

extern "C" {

EXPORT Version get_version();

EXPORT const char *get_plugin_name();

EXPORT void cstring_drop(char *s);

EXPORT Plugin *plugin_new();

EXPORT void plugin_set_state(const Plugin *plugin, const char *state);

EXPORT char *plugin_get_state(const Plugin *plugin);

EXPORT
void plugin_run(const Plugin *plugin,
                float **outputs,
                float sample_rate,
                uintptr_t sample_count,
                bool is_playing,
                uintptr_t current_sample);

EXPORT void plugin_drop(Plugin *plugin);

EXPORT PluginUi *plugin_ui_new(uintptr_t handle, const Plugin *plugin);

EXPORT void plugin_ui_set_size(const PluginUi *plugin_ui, uintptr_t width, uintptr_t height);

EXPORT void plugin_ui_idle(const PluginUi *plugin_ui);

EXPORT void plugin_ui_drop(PluginUi *plugin_ui);

}  // extern "C"

}  // namespace Rust
