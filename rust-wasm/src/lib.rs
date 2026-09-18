//! Stable raw WebAssembly exports for the handwritten browser adapter.

use riscbox::browser_abi;

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
pub extern "C" fn riscbox_run() {
    browser_abi::riscbox_run();
}
