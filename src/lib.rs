mod model;
mod plugin;
mod ui;
mod common;

use std::sync::Arc;
use tokio::sync::Mutex;
use crate::common::RUNTIME;

pub struct Plugin {
    inner: Arc<Mutex<plugin::PluginImpl>>,
}

pub struct PluginUi {
    inner: Arc<Mutex<ui::PluginUiImpl>>,
}

#[no_mangle]
unsafe extern "C" fn plugin_new() -> *mut Plugin {
    Box::into_raw(Box::new(Plugin {
        inner: Arc::new(Mutex::new(plugin::PluginImpl::new())),
    }))
}

#[no_mangle]
unsafe extern "C" fn plugin_drop(plugin: *mut Plugin) {
    if plugin.is_null() {
        return;
    }

    let plugin = Box::from_raw(plugin);
    drop(plugin);
}

#[no_mangle]
unsafe extern "C" fn plugin_ui_new(handle: usize, plugin: &Plugin) -> *mut PluginUi {
    let plugin_ref = Arc::clone(&plugin.inner);
    let plugin_ui = ui::PluginUiImpl::new(handle, plugin_ref);

    Box::into_raw(Box::new(PluginUi {
        inner: Arc::new(Mutex::new(plugin_ui)),
    }))
}

#[no_mangle]
unsafe extern "C" fn plugin_ui_get_native_window_handle(plugin_ui: &PluginUi) -> usize {
    let plugin_ui =RUNTIME.block_on(plugin_ui.inner.lock());
    plugin_ui.get_native_window_handle()
}

#[no_mangle]
unsafe extern "C" fn plugin_ui_set_size(plugin_ui: &PluginUi, width: usize, height: usize) {
    let plugin_ui = RUNTIME.block_on(plugin_ui.inner.lock());
    plugin_ui.set_size(width, height);
}

#[no_mangle]
unsafe extern "C" fn plugin_ui_drop(plugin_ui: *mut PluginUi) {
    if plugin_ui.is_null() {
        return;
    }

    let plugin_ui = Box::from_raw(plugin_ui);
    drop(plugin_ui);
}
