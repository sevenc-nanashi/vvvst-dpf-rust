mod common;
mod manager;
mod model;
mod plugin;
mod saturating_ext;
mod synthesizer;
mod ui;
mod vst_common;

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};
use vst_common::NUM_CHANNELS;

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
unsafe extern "C-unwind" fn get_version() -> Version {
    let version = env!("CARGO_PKG_VERSION");
    let version = semver::Version::parse(version).unwrap();

    Version {
        major: version.major as _,
        minor: version.minor as _,
        patch: version.patch as _,
    }
}

#[no_mangle]
unsafe extern "C-unwind" fn get_plugin_name() -> *const std::os::raw::c_char {
    let name = format!(
        "{}-{}",
        env!("CARGO_PKG_NAME"),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    let name = name.as_bytes();
    let name = std::ffi::CString::new(name).unwrap();
    name.into_raw()
}

#[no_mangle]
unsafe extern "C-unwind" fn cstring_drop(s: *mut std::os::raw::c_char) {
    if s.is_null() {
        return;
    }

    let _ = std::ffi::CString::from_raw(s);
}

#[no_mangle]
unsafe extern "C-unwind" fn plugin_new() -> *mut Plugin {
    Box::into_raw(Box::new(Plugin {
        inner: Arc::new(Mutex::new(plugin::PluginImpl::new(Default::default()))),
    }))
}

#[no_mangle]
unsafe extern "C-unwind" fn plugin_set_state(plugin: &Plugin, state: *const std::ffi::c_char) {
    let plugin = plugin.inner.blocking_lock();
    let state = std::ffi::CStr::from_ptr(state).to_str().unwrap();
    let _ = plugin.set_state(state);
}

#[no_mangle]
unsafe extern "C-unwind" fn plugin_get_state(plugin: &Plugin) -> *mut std::os::raw::c_char {
    let plugin = plugin.inner.blocking_lock();
    let state = plugin.get_state();
    let state = std::ffi::CString::new(state).unwrap();
    state.into_raw()
}

#[no_mangle]
unsafe extern "C-unwind" fn plugin_run(
    plugin: &Plugin,
    outputs: *mut *mut f32,
    sample_rate: f32,
    sample_count: usize,
    is_playing: bool,
    current_sample: i64,
) {
    let mut outputs = std::slice::from_raw_parts_mut(outputs, NUM_CHANNELS as usize)
        .iter_mut()
        .map(|&mut ptr| std::slice::from_raw_parts_mut(ptr, sample_count))
        .collect::<Vec<_>>();

    let plugin_ref = Arc::clone(&plugin.inner);
    plugin::PluginImpl::run(
        plugin_ref,
        &mut outputs,
        sample_rate,
        is_playing,
        current_sample,
    );
}

#[no_mangle]
unsafe extern "C-unwind" fn plugin_drop(plugin: *mut Plugin) {
    if plugin.is_null() {
        return;
    }

    let plugin = Box::from_raw(plugin);
    drop(plugin);
}

#[no_mangle]
unsafe extern "C-unwind" fn plugin_ui_new(
    handle: usize,
    plugin: &Plugin,
    width: usize,
    height: usize,
    scale_factor: f64,
) -> *mut PluginUi {
    let plugin_ref = Arc::clone(&plugin.inner);
    let plugin_ui = match ui::PluginUiImpl::new(handle, plugin_ref, width, height, scale_factor) {
        Ok(plugin_ui) => {
            info!("PluginUi created");
            plugin_ui
        }
        Err(e) => {
            error!("Failed to create PluginUi: {}", e);
            return std::ptr::null_mut();
        }
    };

    Box::into_raw(Box::new(PluginUi {
        inner: Arc::new(Mutex::new(plugin_ui)),
    }))
}

#[no_mangle]
unsafe extern "C-unwind" fn plugin_ui_set_size(
    plugin_ui: &PluginUi,
    width: usize,
    height: usize,
    scale_factor: f64,
) {
    let plugin_ui = plugin_ui.inner.blocking_lock();
    if let Err(err) = plugin_ui.set_size(width, height, scale_factor) {
        error!("Failed to set size: {}", err);
    }
}

#[no_mangle]
unsafe extern "C-unwind" fn plugin_ui_idle(plugin_ui: &PluginUi) {
    let mut plugin_ui = plugin_ui.inner.blocking_lock();
    if let Err(err) = plugin_ui.idle() {
        error!("Idle callback failed: {}", err);
    }
}

#[no_mangle]
unsafe extern "C-unwind" fn plugin_ui_drop(plugin_ui: *mut PluginUi) {
    if plugin_ui.is_null() {
        return;
    }

    let plugin_ui = Box::from_raw(plugin_ui);
    let plugin_ui = match Arc::try_unwrap(plugin_ui.inner) {
        Ok(plugin_ui) => plugin_ui,
        Err(_) => {
            error!("Failed to drop PluginUi: still has references");
            return;
        }
    };
    let plugin_ui = plugin_ui.into_inner();

    match vst_common::RUNTIME.block_on(plugin_ui.terminate()) {
        Ok(_) => info!("PluginUi dropped"),
        Err(e) => error!("Failed to drop PluginUi: {}", e),
    }
}
