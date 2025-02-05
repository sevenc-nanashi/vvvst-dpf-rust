// xtaskによって生成。手動で編集しないでください。
#include "rust_bridge.generated.hpp"
#include "rust_bridge.hpp"

namespace Rust {
typedef Version (*get_version_t)();
Version get_version() {
  auto rust = Rust::loadRustDll();
  auto fn = (get_version_t)rust->findFunction("get_version");
  return fn();
}

typedef const char *(*get_plugin_name_t)();
const char *get_plugin_name() {
  auto rust = Rust::loadRustDll();
  auto fn = (get_plugin_name_t)rust->findFunction("get_plugin_name");
  return fn();
}

typedef void (*cstring_drop_t)(char *s);
void cstring_drop(char *s) {
  auto rust = Rust::loadRustDll();
  auto fn = (cstring_drop_t)rust->findFunction("cstring_drop");
  return fn(s);
}

typedef Plugin *(*plugin_new_t)();
Plugin *plugin_new() {
  auto rust = Rust::loadRustDll();
  auto fn = (plugin_new_t)rust->findFunction("plugin_new");
  return fn();
}

typedef void (*plugin_set_state_t)(const Plugin *plugin, const char *state);
void plugin_set_state(const Plugin *plugin, const char *state) {
  auto rust = Rust::loadRustDll();
  auto fn = (plugin_set_state_t)rust->findFunction("plugin_set_state");
  return fn(plugin, state);
}

typedef char *(*plugin_get_state_t)(const Plugin *plugin);
char *plugin_get_state(const Plugin *plugin) {
  auto rust = Rust::loadRustDll();
  auto fn = (plugin_get_state_t)rust->findFunction("plugin_get_state");
  return fn(plugin);
}

typedef void (*plugin_run_t)(const Plugin *plugin, float **outputs,
                             float sample_rate, uintptr_t sample_count,
                             bool is_playing, int64_t current_sample);
void plugin_run(const Plugin *plugin, float **outputs, float sample_rate,
                uintptr_t sample_count, bool is_playing,
                int64_t current_sample) {
  auto rust = Rust::loadRustDll();
  auto fn = (plugin_run_t)rust->findFunction("plugin_run");
  return fn(plugin, outputs, sample_rate, sample_count, is_playing,
            current_sample);
}

typedef void (*plugin_drop_t)(Plugin *plugin);
void plugin_drop(Plugin *plugin) {
  auto rust = Rust::loadRustDll();
  auto fn = (plugin_drop_t)rust->findFunction("plugin_drop");
  return fn(plugin);
}

typedef PluginUi *(*plugin_ui_new_t)(uintptr_t handle, const Plugin *plugin,
                                     uintptr_t width, uintptr_t height);
PluginUi *plugin_ui_new(uintptr_t handle, const Plugin *plugin, uintptr_t width,
                        uintptr_t height) {
  auto rust = Rust::loadRustDll();
  auto fn = (plugin_ui_new_t)rust->findFunction("plugin_ui_new");
  return fn(handle, plugin, width, height);
}

typedef void (*plugin_ui_set_size_t)(const PluginUi *plugin_ui, uintptr_t width,
                                     uintptr_t height);
void plugin_ui_set_size(const PluginUi *plugin_ui, uintptr_t width,
                        uintptr_t height) {
  auto rust = Rust::loadRustDll();
  auto fn = (plugin_ui_set_size_t)rust->findFunction("plugin_ui_set_size");
  return fn(plugin_ui, width, height);
}

typedef void (*plugin_ui_idle_t)(const PluginUi *plugin_ui);
void plugin_ui_idle(const PluginUi *plugin_ui) {
  auto rust = Rust::loadRustDll();
  auto fn = (plugin_ui_idle_t)rust->findFunction("plugin_ui_idle");
  return fn(plugin_ui);
}

typedef void (*plugin_ui_drop_t)(PluginUi *plugin_ui);
void plugin_ui_drop(PluginUi *plugin_ui) {
  auto rust = Rust::loadRustDll();
  auto fn = (plugin_ui_drop_t)rust->findFunction("plugin_ui_drop");
  return fn(plugin_ui);
}

} // namespace Rust
