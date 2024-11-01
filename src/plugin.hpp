#pragma once
#include "DistrhoPlugin.hpp"
#include "extra/String.hpp"
#include "rust.generated.hpp"
#include <memory>

START_NAMESPACE_DISTRHO

class VvvstPlugin : public Plugin {
public:
  VvvstPlugin();

  std::shared_ptr<Rust::Plugin> inner;

protected:
  /* --------------------------------------------------------------------------------------------------------
   * Information */

  /**
     Get the plugin label.
     A plugin label follows the same rules as Parameter::symbol, with the
     exception that it can start with numbers.
   */
  const char *getLabel() const override;

  /**
     Get an extensive comment/description about the plugin.
   */
  const char *getDescription() const override;

  /**
     Get the plugin author/maker.
   */
  const char *getMaker() const override;

  /**
     Get the plugin homepage.
   */
  const char *getHomePage() const override;

  /**
     Get the plugin license name (a single line of text).
     For commercial plugins this should return some short copyright information.
   */
  const char *getLicense() const override;

  /**
     Get the plugin version, in hexadecimal.
   */
  uint32_t getVersion() const override;

  /* --------------------------------------------------------------------------------------------------------
   * Init */

  /**
     Initialize the audio port @a index.@n
     This function will be called once, shortly after the plugin is created.
   */
  void initAudioPort(bool input, uint32_t index, AudioPort &port) override;

  void initState(uint32_t index, State &state) override;
  String getState(const char *key) const override;
  void setState(const char *key, const char *value) override;

  /* --------------------------------------------------------------------------------------------------------
   * Process */

  /**
     Run/process function for plugins without MIDI input.
   */
  void run(const float **inputs, float **outputs, uint32_t frames,
           const MidiEvent *midiEvents, uint32_t midiEventCount) override;

  // -------------------------------------------------------------------------------------------------------

private:
  /**
     Set our plugin class as non-copyable and add a leak detector just in case.
   */
  DISTRHO_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(VvvstPlugin)
};

END_NAMESPACE_DISTRHO
