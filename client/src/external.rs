use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::sync::{Arc, Mutex};

use cef::types::list::List;
use cef_sys::cef_list_value_t;

use crossbeam_channel::Sender;

use crate::app::Event;
use crate::browser::manager::Manager;
use std::process::exit;

static mut PLUGINS: Option<ExternalManager> = None;

pub type EventCallback = extern "C" fn(*const c_char, *mut cef_list_value_t) -> c_int;
pub type CallbackList = Arc<Mutex<HashMap<String, EventCallback>>>;

pub const EXTERNAL_CONTINUE: c_int = 0;
pub const EXTERNAL_BREAK: c_int = 1;

pub struct ExternalManager {
    manager: Arc<Mutex<Manager>>,
    event_tx: Sender<Event>,
    callbacks: CallbackList,
}

impl ExternalManager {
    pub fn get<'a>() -> Option<&'a mut ExternalManager> {
        unsafe { PLUGINS.as_mut() }
    }
}

pub fn initialize(event_tx: Sender<Event>, manager: Arc<Mutex<Manager>>) -> CallbackList {
    let callbacks = Arc::new(Mutex::new(HashMap::new()));

    let external = ExternalManager {
        manager,
        event_tx,
        callbacks: callbacks.clone(),
    };

    unsafe {
        PLUGINS = Some(external);
    }

    callbacks
}

#[no_mangle]
pub unsafe extern "C" fn cef_create_browser(id: u32, url: *const c_char) {
    if url.is_null() {
        return;
    }

    if let Some(external) = ExternalManager::get() {
        let url = CStr::from_ptr(url);
        let url_rust = url.to_string_lossy();

        let event = Event::CreateBrowser {
            id,
            url: url_rust.to_string(),
            listen_events: false,
        };

        external.event_tx.send(event);
    }
}

#[no_mangle]
pub unsafe extern "C" fn cef_destroy_browser(id: u32) {
    if let Some(external) = ExternalManager::get() {
        let mut manager = external.manager.lock().unwrap();
        manager.close_browser(id, true);
    }
}

#[no_mangle]
pub unsafe extern "C" fn cef_hide_browser(id: u32, hide: bool) {
    if let Some(external) = ExternalManager::get() {
        let manager = external.manager.lock().unwrap();
        manager.hide_browser(id, hide);
    }
}

#[no_mangle]
pub unsafe extern "C" fn cef_focus_browser(id: u32, focus: bool) {
    if let Some(external) = ExternalManager::get() {
        {
            let mut manager = external.manager.lock().unwrap();
            manager.browser_focus(id, focus);
        }

        let event = Event::FocusBrowser(id, focus);
        external.event_tx.send(event);
    }
}

#[no_mangle]
pub unsafe extern "C" fn cef_create_list() -> *mut cef_list_value_t {
    let list = List::new();
    list.into_cef()
}

#[no_mangle]
pub unsafe extern "C" fn cef_emit_event(event: *const c_char, list: *mut cef_list_value_t) {
    if event.is_null() {
        return;
    }

    if let Some(list) = List::try_from_raw(list) {
        if let Some(external) = ExternalManager::get() {
            let manager = external.manager.lock().unwrap();
            let name = CStr::from_ptr(event);
            manager.trigger_event(&name.to_string_lossy(), list);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn cef_subscribe(event: *const c_char, callback: Option<EventCallback>) {
    if event.is_null() || callback.is_none() {
        return;
    }

    let event = CStr::from_ptr(event);
    let event_rust = event.to_string_lossy();

    if let Some(external) = ExternalManager::get() {
        let mut cbs = external.callbacks.lock().unwrap();
        cbs.insert(event_rust.to_string(), callback.unwrap());
    }
}

#[no_mangle]
pub unsafe extern "C" fn cef_input_available(browser: u32) -> bool {
    ExternalManager::get()
        .map(|ext| {
            let manager = ext.manager.lock().unwrap();
            manager.is_input_available(browser)
        })
        .unwrap_or(false)
}

#[no_mangle]
pub unsafe extern "C" fn cef_ready() -> bool {
    false
}

#[no_mangle]
pub unsafe extern "C" fn cef_try_focus_browser(browser: u32) -> bool {
    ExternalManager::get()
        .map(|ext| {
            let mut manager = ext.manager.lock().unwrap();

            if manager.is_input_available(browser) {
                manager.browser_focus(browser, true);
                drop(manager);

                let event = Event::FocusBrowser(browser, true);
                ext.event_tx.send(event);

                true
            } else {
                false
            }
        })
        .unwrap_or(false)
}
