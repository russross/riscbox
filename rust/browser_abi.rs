//! Safe storage and exported scalar calls for the raw WebAssembly adapter.

use std::cell::RefCell;

use crate::browser::BrowserController;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartRequest {
    pub config_url: String,
    pub ram_mib: u32,
    pub command_line: String,
    pub password: String,
    pub width: u32,
    pub height: u32,
    pub has_network: bool,
}

#[derive(Default)]
struct AbiState {
    allocations: Vec<Box<[u8]>>,
    controller: BrowserController,
    start: Option<StartRequest>,
}

thread_local! {
    static STATE: RefCell<AbiState> = RefCell::new(AbiState::default());
}

#[must_use]
pub extern "C" fn riscbox_alloc(length: u32) -> u32 {
    let Ok(length) = usize::try_from(length) else {
        return 0;
    };
    if length == 0 {
        return 0;
    }
    STATE.with_borrow_mut(|state| {
        let allocation = vec![0; length].into_boxed_slice();
        let address = allocation.as_ptr() as usize;
        let Ok(address) = u32::try_from(address) else {
            return 0;
        };
        state.allocations.push(allocation);
        address
    })
}

pub extern "C" fn riscbox_free(address: u32, length: u32) {
    STATE.with_borrow_mut(|state| {
        if let Some(index) = state.allocations.iter().position(|allocation| {
            allocation.as_ptr() as usize == address as usize && allocation.len() == length as usize
        }) {
            state.allocations.swap_remove(index);
        }
    });
}

#[must_use]
pub extern "C" fn riscbox_start(
    url_address: u32,
    url_length: u32,
    ram_mib: u32,
    command_address: u32,
    command_length: u32,
    password_address: u32,
    password_length: u32,
    width: u32,
    height: u32,
    has_network: u32,
) -> i32 {
    STATE.with_borrow_mut(|state| {
        let Some(config_url) = allocated_string(state, url_address, url_length) else {
            return -1;
        };
        let Some(command_line) = allocated_string(state, command_address, command_length) else {
            return -1;
        };
        let Some(password) = allocated_string(state, password_address, password_length) else {
            return -1;
        };
        if config_url.is_empty() || ram_mib == 0 {
            return -1;
        }
        state.start = Some(StartRequest {
            config_url,
            ram_mib,
            command_line,
            password,
            width,
            height,
            has_network: has_network != 0,
        });
        0
    })
}

#[must_use]
pub extern "C" fn riscbox_console_input(address: u32, length: u32) -> u32 {
    STATE.with_borrow_mut(|state| {
        let Some(bytes) = allocated_bytes(state, address, length).map(<[u8]>::to_vec) else {
            return 0;
        };
        u32::try_from(state.controller.queue_console(&bytes)).unwrap_or(0)
    })
}

#[must_use]
pub extern "C" fn riscbox_console_resize(columns: u32, rows: u32) -> i32 {
    let (Ok(columns), Ok(rows)) = (u16::try_from(columns), u16::try_from(rows)) else {
        return -1;
    };
    STATE.with_borrow_mut(|state| state.controller.resize(columns, rows));
    0
}

#[must_use]
pub extern "C" fn riscbox_key_event(pressed: u32, code: u32) -> i32 {
    let Ok(code) = u16::try_from(code) else {
        return -1;
    };
    STATE.with_borrow_mut(|state| state.controller.key_event(pressed != 0, code));
    0
}

#[must_use]
pub extern "C" fn riscbox_pointer_event(x: u32, y: u32, buttons: u32) -> i32 {
    STATE.with_borrow_mut(|state| state.controller.pointer_event(x, y, buttons));
    0
}

#[must_use]
pub extern "C" fn riscbox_wheel_event(delta: i32) -> i32 {
    STATE.with_borrow_mut(|state| state.controller.wheel_event(delta));
    0
}

#[must_use]
pub extern "C" fn riscbox_network_input(address: u32, length: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        let Some(bytes) = allocated_bytes(state, address, length).map(<[u8]>::to_vec) else {
            return -1;
        };
        state.controller.network_packet(&bytes);
        0
    })
}

#[must_use]
pub extern "C" fn riscbox_network_carrier(up: u32) -> i32 {
    STATE.with_borrow_mut(|state| state.controller.network_carrier(up != 0));
    0
}

pub extern "C" fn riscbox_run() {}

#[must_use]
pub fn take_start_request() -> Option<StartRequest> {
    STATE.with_borrow_mut(|state| state.start.take())
}

pub fn with_controller<T>(callback: impl FnOnce(&mut BrowserController) -> T) -> T {
    STATE.with_borrow_mut(|state| callback(&mut state.controller))
}

fn allocated_bytes(state: &AbiState, address: u32, length: u32) -> Option<&[u8]> {
    state
        .allocations
        .iter()
        .find(|allocation| allocation.as_ptr() as usize == address as usize)
        .and_then(|allocation| allocation.get(..length as usize))
}

fn allocated_string(state: &AbiState, address: u32, length: u32) -> Option<String> {
    if length == 0 {
        return Some(String::new());
    }
    String::from_utf8(allocated_bytes(state, address, length)?.to_vec()).ok()
}
