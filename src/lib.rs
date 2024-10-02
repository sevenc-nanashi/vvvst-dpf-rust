mod common;
mod model;
mod plugin;
mod ui;

use crate::common::RUNTIME;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Plugin {
    inner: Arc<Mutex<plugin::PluginImpl>>,
}

pub struct PluginUi {
    inner: Arc<Mutex<ui::PluginUiImpl>>,
}

#[repr(C)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

#[no_mangle]
unsafe extern "C" fn get_version() -> Version {
    let version = env!("CARGO_PKG_VERSION");
    let version_split = version.split('.').collect::<Vec<_>>();
    let major = version_split[0].parse::<u8>().unwrap();
    let minor = version_split[1].parse::<u8>().unwrap();
    let patch = version_split[2].parse::<u8>().unwrap();

    Version {
        major,
        minor,
        patch,
    }
}

#[no_mangle]
unsafe extern "C" fn get_plugin_name() -> *const std::os::raw::c_char {
    let name = env!("CARGO_PKG_NAME");
    let name = name.as_bytes();
    let name = std::ffi::CString::new(name).unwrap();
    name.into_raw()
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
    let plugin_ui = RUNTIME.block_on(plugin_ui.inner.lock());
    plugin_ui.get_native_window_handle()
}

#[no_mangle]
unsafe extern "C" fn plugin_ui_set_size(plugin_ui: &PluginUi, width: usize, height: usize) {
    let plugin_ui = RUNTIME.block_on(plugin_ui.inner.lock());
    plugin_ui.set_size(width, height);
}

#[no_mangle]
unsafe extern "C" fn plugin_ui_idle(plugin_ui: &PluginUi) {
    let mut plugin_ui = RUNTIME.block_on(plugin_ui.inner.lock());
    plugin_ui.idle();
}

#[no_mangle]
unsafe extern "C" fn plugin_ui_drop(plugin_ui: *mut PluginUi) {
    if plugin_ui.is_null() {
        return;
    }

    let plugin_ui = Box::from_raw(plugin_ui);
    drop(plugin_ui);
}
