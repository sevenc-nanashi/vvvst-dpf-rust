#include "plugin.hpp"
#include "DistrhoDetails.hpp"
#include "DistrhoPlugin.hpp"
#include "rust.generated.hpp"
// -----------------------------------------------------------------------------------------------------------

VvvstPlugin::VvvstPlugin() : Plugin(0, 0, 1) { inner = Rust::plugin_new(); }
VvvstPlugin::~VvvstPlugin() { Rust::plugin_drop(inner); }

/**
   Get the plugin label.
   A plugin label follows the same rules as Parameter::symbol, with the
   exception that it can start with numbers.
 */
const char *VvvstPlugin::getLabel() const { return "vvvst"; }

/**
   Get an extensive comment/description about the plugin.
 */
const char *VvvstPlugin::getDescription() const {
  return "VOICEVOXのVSTプラグインです。";
}

/**
   Get the plugin author/maker.
 */
const char *VvvstPlugin::getMaker() const {
  return "Nanashi. <https://sevenc7c.com>";
}

/**
   Get the plugin homepage.
 */
const char *VvvstPlugin::getHomePage() const {
  return "https://github.com/sevenc-nanashi/vvvst-rust-dpf";
}

/**
   Get the plugin license name (a single line of text).
   For commercial plugins this should return some short copyright information.
 */
const char *VvvstPlugin::getLicense() const { return "LGPLv3"; }

/**
   Get the plugin version, in hexadecimal.
 */
uint32_t VvvstPlugin::getVersion() const {
  auto version = Rust::get_version();

  return d_version(version.major, version.minor, version.patch);
}

/* --------------------------------------------------------------------------------------------------------
 * Init */

/**
   Initialize the audio port @a index.@n
   This function will be called once, shortly after the plugin is created.
 */
void VvvstPlugin::initAudioPort(bool input, uint32_t index, AudioPort &port) {
  // treat meter audio ports as stereo
  port.groupId = kPortGroupStereo;

  // everything else is as default
  Plugin::initAudioPort(input, index, port);
}

void VvvstPlugin::initState(uint32_t index, State &state) {
  state.defaultValue = "";
  state.key = "state";
  state.hints = kStateIsBase64Blob;
}
void VvvstPlugin::setState(const char *key, const char *value) {
  Rust::plugin_set_state(inner, value);
}
String VvvstPlugin::getState(const char *key) const {
  auto stateStringPtr = Rust::plugin_get_state(inner);
  auto stateStdString = std::string(stateStringPtr);
  Rust::cstring_drop(stateStringPtr);

  return String(stateStdString.c_str());
}

/* --------------------------------------------------------------------------------------------------------
 * Process */

/**
   Run/process function for plugins without MIDI input.
 */
void VvvstPlugin::run(const float **inputs, float **outputs, uint32_t frames) {
  auto sampleRate = this->getSampleRate();
  auto timePosition = this->getTimePosition();
  auto samplePosition = timePosition.frame;
  auto isPlaying = timePosition.playing;
  Rust::plugin_run(inner, outputs, sampleRate, frames, isPlaying,
                   samplePosition);
}

START_NAMESPACE_DISTRHO
Plugin *createPlugin() { return new VvvstPlugin(); }
END_NAMESPACE_DISTRHO
