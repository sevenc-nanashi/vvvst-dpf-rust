// xtaskによって生成。手動で編集しないでください。
#pragma once
#include <choc/platform/choc_DynamicLibrary.h>
#include <cstdint>

namespace Rust {

struct Plugin;

struct PluginUi;

struct Version {
  uint8_t major;
  uint8_t minor;
  uint8_t patch;
};

Version get_version();

const char *get_plugin_name();

void cstring_drop(char *s);

Plugin *plugin_new();

void plugin_set_state(const Plugin *plugin, const char *state);

char *plugin_get_state(const Plugin *plugin);

void plugin_run(const Plugin *plugin, float **outputs, float sample_rate,
                uintptr_t sample_count, bool is_playing,
                int64_t current_sample);

void plugin_drop(Plugin *plugin);

PluginUi *plugin_ui_new(uintptr_t handle, const Plugin *plugin, uintptr_t width,
                        uintptr_t height, double scale_factor);

void plugin_ui_set_size(const PluginUi *plugin_ui, uintptr_t width,
                        uintptr_t height, double scale_factor);

void plugin_ui_idle(const PluginUi *plugin_ui);

void plugin_ui_drop(PluginUi *plugin_ui);

} // namespace Rust
