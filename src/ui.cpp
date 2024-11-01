#include "DistrhoUI.hpp"
#include "plugin.hpp"
#include "rust.generated.hpp"
#include <memory>
#include <mutex>

START_NAMESPACE_DISTRHO

// -----------------------------------------------------------------------------------------------------------

class VvvstUi : public UI {
public:
  VvvstUi() : UI() { initializeRustUi(); }
  void parameterChanged(uint32_t index, float value) override {}

  void sizeChanged(uint width, uint height) override {
    auto lock = std::unique_lock(this->mutex);
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
      uiRetried = true;
      return;
    }
    auto lock = std::unique_lock(this->mutex, std::defer_lock);
    if (lock.try_lock()) {
      Rust::plugin_ui_idle(inner.get());
    }
  }

  void stateChanged(const char *key, const char *value) override {}

  void onSizeChanged(uint width, uint height) {
    if (!inner) {
      return;
    }
    Rust::plugin_ui_set_size(inner.get(), width, height);
  }

private:
  std::mutex mutex;
  std::shared_ptr<Rust::PluginUi> inner;
  bool uiRetried = false;

  void initializeRustUi() {
    auto lock = std::unique_lock(this->mutex);
    if (inner) {
      return;
    }
    auto plugin = static_cast<VvvstPlugin *>(this->getPluginInstancePointer());
    inner = std::shared_ptr<Rust::PluginUi>(
        Rust::plugin_ui_new(this->getParentWindowHandle(), plugin->inner.get()),
        [](Rust::PluginUi *inner) { Rust::plugin_ui_drop(inner); });
    if (!inner) {
      return;
    }
    Rust::plugin_ui_set_size(inner.get(), this->getWidth(), this->getHeight());
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
