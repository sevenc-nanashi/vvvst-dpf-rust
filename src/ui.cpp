#include "DistrhoUI.hpp"
#include "plugin.hpp"
#include "rust.generated.hpp"

START_NAMESPACE_DISTRHO

// -----------------------------------------------------------------------------------------------------------

class VvvstUi : public UI {
public:
  VvvstUi() : UI() {
    initializeRustUi();
  }
  ~VvvstUi() override {
    if (!inner) {
      return;
    }
    Rust::plugin_ui_drop(inner);
  };

  void parameterChanged(uint32_t index, float value) override {}

  void sizeChanged(uint width, uint height) override {
    UI::sizeChanged(width, height);
    onSizeChanged(width, height);
  }

  void uiIdle() override {
    if (!inner) {
      if (uiRetried) {
        return;
      }

      // Cubaseだとコンストラクト直後にRust側を初期化すると失敗することがあるので、1回だけリトライする
      initializeRustUi();
      return;
    }
    Rust::plugin_ui_idle(inner);
  }

  void stateChanged(const char *key, const char *value) override {}

  void onSizeChanged(uint width, uint height) {
    if (!inner) {
      return;
    }
    Rust::plugin_ui_set_size(inner, width, height);
  }

private:
  Rust::PluginUi *inner;
  bool uiRetried = false;

  void initializeRustUi() {
    auto plugin = static_cast<VvvstPlugin *>(this->getPluginInstancePointer());
    inner = Rust::plugin_ui_new(this->getParentWindowHandle(), plugin->inner);
    if (!inner) {
      return;
    }
    sizeChanged(this->getWidth(), this->getHeight());
  }

  /**
     Set our UI class as non-copyable and add a leak detector just in case.
   */
  DISTRHO_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(VvvstUi)
};

/* ------------------------------------------------------------------------------------------------------------
 * UI entry point, called by DPF to create a new UI instance. */

UI *createUI() { return new VvvstUi(); }

// -----------------------------------------------------------------------------------------------------------

END_NAMESPACE_DISTRHO
