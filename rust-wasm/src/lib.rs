//! Stable raw WebAssembly exports for the handwritten browser adapter.

use std::cell::RefCell;
use std::rc::Rc;

use riscbox::browser_abi;
use riscbox::entropy::EntropyError;
use riscbox::virtio_devices::DeviceError;

const P9_REPLY_CAPACITY: usize = 64 * 1024;

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "riscbox_host")]
unsafe extern "C" {
    fn p9_request(request: u32, request_length: u32, reply: u32, reply_capacity: u32) -> i32;
    fn random_fill(destination: u32, length: u32) -> i32;
}

fn install_entropy_callback() {
    browser_abi::set_entropy_callback(Rc::new(RefCell::new(|destination: &mut [u8]| {
        #[cfg(target_arch = "wasm32")]
        let status =
            unsafe { random_fill(destination.as_mut_ptr() as u32, destination.len() as u32) };
        #[cfg(not(target_arch = "wasm32"))]
        let status = {
            let _ = destination;
            -1
        };
        if status == 0 {
            Ok(())
        } else {
            Err(EntropyError)
        }
    })));
}

fn install_ninep_callback() {
    browser_abi::set_ninep_callback(Rc::new(RefCell::new(|request_bytes: &[u8]| {
        let mut reply = vec![0; P9_REPLY_CAPACITY];
        #[cfg(target_arch = "wasm32")]
        let length = unsafe {
            p9_request(
                request_bytes.as_ptr() as u32,
                request_bytes.len() as u32,
                reply.as_mut_ptr() as u32,
                reply.len() as u32,
            )
        };
        #[cfg(not(target_arch = "wasm32"))]
        let length = {
            let _ = request_bytes;
            -1
        };
        let length = usize::try_from(length).map_err(|_| DeviceError::Backend)?;
        if length > reply.len() {
            return Err(DeviceError::Backend);
        }
        reply.truncate(length);
        Ok(reply)
    })));
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_alloc(length: u32) -> u32 {
    browser_abi::riscbox_alloc(length)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_free(address: u32, length: u32) {
    browser_abi::riscbox_free(address, length);
}

#[unsafe(no_mangle)]
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
    install_ninep_callback();
    install_entropy_callback();
    browser_abi::riscbox_start(
        url_address,
        url_length,
        ram_mib,
        command_address,
        command_length,
        password_address,
        password_length,
        width,
        height,
        has_network,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_console_input(address: u32, length: u32) -> u32 {
    browser_abi::riscbox_console_input(address, length)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_console_resize(columns: u32, rows: u32) -> i32 {
    browser_abi::riscbox_console_resize(columns, rows)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_key_event(pressed: u32, code: u32) -> i32 {
    browser_abi::riscbox_key_event(pressed, code)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_pointer_event(x: u32, y: u32, buttons: u32) -> i32 {
    browser_abi::riscbox_pointer_event(x, y, buttons)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_wheel_event(delta: i32) -> i32 {
    browser_abi::riscbox_wheel_event(delta)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_network_input(address: u32, length: u32) -> i32 {
    browser_abi::riscbox_network_input(address, length)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_network_carrier(up: u32) -> i32 {
    browser_abi::riscbox_network_carrier(up)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_run(now_milliseconds_low: u32, now_milliseconds_high: u32) -> i32 {
    browser_abi::riscbox_run(now_milliseconds_low, now_milliseconds_high)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_next_action() -> u32 {
    browser_abi::riscbox_next_action()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_action_value() -> u32 {
    browser_abi::riscbox_action_value()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_action_data_address() -> u32 {
    browser_abi::riscbox_action_data_address()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_action_data_length() -> u32 {
    browser_abi::riscbox_action_data_length()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_action_x() -> u32 {
    browser_abi::riscbox_action_x()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_action_y() -> u32 {
    browser_abi::riscbox_action_y()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_action_width() -> u32 {
    browser_abi::riscbox_action_width()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_action_height() -> u32 {
    browser_abi::riscbox_action_height()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_action_stride() -> u32 {
    browser_abi::riscbox_action_stride()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_http_complete(id: u32, status: u32, address: u32, length: u32) -> i32 {
    browser_abi::riscbox_http_complete(id, status, address, length)
}
