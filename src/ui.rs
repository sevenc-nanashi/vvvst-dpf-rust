use crate::plugin::{PluginImpl, ToPluginMessage};
use crate::utils::{isize_to_usize, usize_to_isize};
use std::{
    num::NonZeroIsize,
    sync::{mpsc::Sender, Arc, Mutex, Weak},
};

pub struct PluginUiImpl {
    raw_window_handle: raw_window_handle::RawWindowHandle,
    window: wry::WebView,

    plugin_sender: Sender<ToPluginMessage>,
}

impl PluginUiImpl {
    pub unsafe fn new(handle: usize, plugin: &mut PluginImpl) -> Self {
        let raw_window_handle =
            raw_window_handle::RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(
                NonZeroIsize::new(usize_to_isize(handle)).unwrap(),
            ));
        let window_handle = raw_window_handle::WindowHandle::borrow_raw(raw_window_handle);

        let window = wry::WebViewBuilder::new(&window_handle).build().unwrap();

        let (sender, receiver) = std::sync::mpsc::channel();
        plugin.receiver = Some(receiver);

        PluginUiImpl {
            raw_window_handle,
            window,

            plugin_sender: sender,
        }
    }

    pub fn get_native_window_handle(&self) -> usize {
        match self.raw_window_handle {
            raw_window_handle::RawWindowHandle::Win32(handle) => isize_to_usize(handle.hwnd.get()),
            _ => 0,
        }
    }

    pub fn set_size(&self, width: usize, height: usize) {
        self.window
            .set_bounds(wry::Rect {
                position: winit::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: winit::dpi::LogicalSize::new(width as f64, height as f64).into(),
            })
            .unwrap();
    }
}
