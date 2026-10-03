//! Stable raw WebAssembly exports for the handwritten browser adapter.

use std::cell::RefCell;
use std::rc::Rc;

use riscbox::entropy::EntropyError;
use riscbox::{block_abi, browser_abi, filesystem_abi};

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "riscbox_host")]
unsafe extern "C" {
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

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_alloc(length: u32) -> u32 {
    browser_abi::riscbox_alloc(length)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_free(address: u32, length: u32) {
    browser_abi::riscbox_free(address, length);
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_fs_call(handle: u32, address: u32, length: u32) -> i32 {
    filesystem_abi::riscbox_fs_call(handle, address, length)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_fs_next_change(handle: u32) -> i32 {
    filesystem_abi::riscbox_fs_next_change(handle)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_fs_status() -> i32 {
    filesystem_abi::riscbox_fs_status()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_fs_data_address() -> u32 {
    filesystem_abi::riscbox_fs_data_address()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_fs_data_length() -> u32 {
    filesystem_abi::riscbox_fs_data_length()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_start(
    url_address: u32,
    url_length: u32,
    ram_mib: u32,
    command_address: u32,
    command_length: u32,
    width: u32,
    height: u32,
    has_network: u32,
) -> i32 {
    install_entropy_callback();
    browser_abi::riscbox_start(
        url_address,
        url_length,
        ram_mib,
        command_address,
        command_length,
        width,
        height,
        has_network,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_start_resolved(
    config_address: u32,
    config_length: u32,
    ram_mib: u32,
    width: u32,
    height: u32,
    has_network: u32,
) -> i32 {
    install_entropy_callback();
    browser_abi::riscbox_start_resolved(
        config_address,
        config_length,
        ram_mib,
        width,
        height,
        has_network,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_halt() -> i32 {
    browser_abi::riscbox_halt()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_reset() -> i32 {
    browser_abi::riscbox_reset()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_destroy() -> i32 {
    browser_abi::riscbox_destroy()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_request_shutdown() -> i32 {
    browser_abi::riscbox_request_shutdown()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_request_reboot() -> i32 {
    browser_abi::riscbox_request_reboot()
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
pub extern "C" fn riscbox_configure_quantum(target_quantum_ms: f64, diagnostics: u32) -> i32 {
    browser_abi::riscbox_configure_quantum(target_quantum_ms, diagnostics)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_wake_delay_ms(
    host_epoch_ms_low: u32,
    host_epoch_ms_high: u32,
    requested: u32,
) -> u32 {
    browser_abi::riscbox_wake_delay_ms(host_epoch_ms_low, host_epoch_ms_high, requested)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_quantum_begin(host_epoch_ms_low: u32, host_epoch_ms_high: u32) -> i32 {
    browser_abi::riscbox_quantum_begin(host_epoch_ms_low, host_epoch_ms_high)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_quantum_run() -> i32 {
    browser_abi::riscbox_quantum_run()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_quantum_finish(
    host_elapsed_ms: f64,
    host_epoch_ms_low: u32,
    host_epoch_ms_high: u32,
) -> i32 {
    browser_abi::riscbox_quantum_finish(host_elapsed_ms, host_epoch_ms_low, host_epoch_ms_high)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_quantum_abort() {
    browser_abi::riscbox_quantum_abort();
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_timing_stat(kind: u32) -> f64 {
    browser_abi::riscbox_timing_stat(kind)
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

// Share lookup does not require a separate construction or binding phase.
#[unsafe(no_mangle)]
pub extern "C" fn riscbox_fs_get(address: u32, length: u32) -> u32 {
    filesystem_abi::riscbox_fs_get(address, length)
}

// Platform storage and preparation use the same copied raw ABI boundary.
#[unsafe(no_mangle)]
pub extern "C" fn riscbox_prepare_resolved(
    config_address: u32,
    config_length: u32,
    ram_mib: u32,
    width: u32,
    height: u32,
    has_network: u32,
) -> i32 {
    install_entropy_callback();
    browser_abi::riscbox_prepare_resolved(
        config_address,
        config_length,
        ram_mib,
        width,
        height,
        has_network,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_cold_reset() -> i32 {
    browser_abi::riscbox_cold_reset()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_action_disk() -> u32 {
    browser_abi::riscbox_action_disk()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_disk_read(disk: u32, low: u32, high: u32, length: u32) -> i32 {
    block_abi::riscbox_disk_read(disk, low, high, length)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_disk_finish(id: u32) -> i32 {
    block_abi::riscbox_disk_finish(id)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_disk_write(
    disk: u32,
    low: u32,
    high: u32,
    address: u32,
    length: u32,
) -> i32 {
    block_abi::riscbox_disk_write(disk, low, high, address, length)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_disk_discard(disk: u32) -> i32 {
    block_abi::riscbox_disk_discard(disk)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_disk_capacity(disk: u32, high: u32) -> u32 {
    block_abi::riscbox_disk_capacity(disk, high)
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_disk_data_address() -> u32 {
    block_abi::riscbox_disk_data_address()
}

#[unsafe(no_mangle)]
pub extern "C" fn riscbox_disk_data_length() -> u32 {
    block_abi::riscbox_disk_data_length()
}
