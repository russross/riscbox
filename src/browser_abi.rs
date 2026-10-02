//! Safe storage and exported scalar calls for the raw WebAssembly adapter.

use std::cell::RefCell;

use crate::browser_input::{BrowserInputQueue, NetworkInputResult};
use crate::browser_runtime::{
    BrowserRuntime, EntropyCallback, HostAction, QuantumOutcome, QuantumStart, RuntimeStart,
};
use crate::config::VmConfig;

#[path = "browser_abi/block.rs"]
pub mod block;
#[path = "browser_abi/ninep.rs"]
pub mod ninep;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartRequest {
    pub config_url: String,
    pub ram_mib: u32,
    pub command_line: String,
    pub width: u32,
    pub height: u32,
    pub has_network: bool,
}

#[derive(Default)]
struct AbiState {
    allocations: Vec<Box<[u8]>>,
    input_queue: BrowserInputQueue,
    start: Option<StartRequest>,
    runtime: BrowserRuntime,
    action: Option<HostAction>,
    filesystems: ninep::FilesystemAbi,
    blocks: block::BlockAbi,
}

thread_local! {
    static STATE: RefCell<AbiState> = RefCell::new(AbiState::default());
}

pub fn set_entropy_callback(callback: EntropyCallback) {
    STATE.with_borrow_mut(|state| state.runtime.set_entropy_callback(callback));
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
        if config_url.is_empty() || ram_mib == 0 {
            return -1;
        }
        let request = RuntimeStart {
            config_url: config_url.clone(),
            ram_mib,
            command_line: command_line.clone(),
            width,
            height,
            has_network: has_network != 0,
        };
        state.start = Some(StartRequest {
            config_url,
            ram_mib,
            command_line,
            width,
            height,
            has_network: has_network != 0,
        });
        if state.runtime.start(request).is_err() {
            -1
        } else {
            0
        }
    })
}

#[must_use]
pub extern "C" fn riscbox_start_resolved(
    config_address: u32,
    config_length: u32,
    ram_mib: u32,
    width: u32,
    height: u32,
    has_network: u32,
) -> i32 {
    start_resolved(
        config_address,
        config_length,
        ram_mib,
        width,
        height,
        has_network,
        false,
    )
}

#[must_use]
pub extern "C" fn riscbox_prepare_resolved(
    config_address: u32,
    config_length: u32,
    ram_mib: u32,
    width: u32,
    height: u32,
    has_network: u32,
) -> i32 {
    start_resolved(
        config_address,
        config_length,
        ram_mib,
        width,
        height,
        has_network,
        true,
    )
}

fn start_resolved(
    config_address: u32,
    config_length: u32,
    ram_mib: u32,
    width: u32,
    height: u32,
    has_network: u32,
    prepare: bool,
) -> i32 {
    STATE.with_borrow_mut(|state| {
        let Some(source) = allocated_string(state, config_address, config_length) else {
            return -1;
        };
        let Ok(config) = VmConfig::from_resolved(&source) else {
            return -1;
        };
        let memory = if ram_mib == 0 {
            u32::try_from(config.memory_size_mib).unwrap_or(0)
        } else {
            ram_mib
        };
        if memory == 0 {
            return -1;
        }
        let start = RuntimeStart {
            config_url: String::new(),
            ram_mib: memory,
            command_line: String::new(),
            width,
            height,
            has_network: has_network != 0,
        };
        if prepare {
            state.runtime.prepare_resolved(start, config)
        } else {
            state.runtime.start_resolved(start, config)
        }
        .map_or(-1, |()| 0)
    })
}

#[must_use]
pub extern "C" fn riscbox_halt() -> i32 {
    STATE.with_borrow_mut(|state| state.runtime.halt().map_or(-1, |()| 0))
}

#[must_use]
pub extern "C" fn riscbox_reset() -> i32 {
    STATE.with_borrow_mut(|state| {
        if state.blocks.has_pending() {
            return -1;
        }
        if state.runtime.reset().is_err() {
            return -1;
        }
        state.input_queue = BrowserInputQueue::default();
        0
    })
}

#[must_use]
pub extern "C" fn riscbox_destroy() -> i32 {
    STATE.with_borrow_mut(|state| {
        if state.runtime.destroy().is_err() {
            return -1;
        }
        state.filesystems.destroy();
        state.blocks.retire(None);
        state.input_queue = BrowserInputQueue::default();
        0
    })
}

#[must_use]
pub extern "C" fn riscbox_cold_reset() -> i32 {
    STATE.with_borrow_mut(|state| {
        if state.runtime.cold_reset().is_err() {
            return -1;
        }
        state.blocks.retire(None);
        state.input_queue = BrowserInputQueue::default();
        0
    })
}

#[must_use]
pub extern "C" fn riscbox_request_shutdown() -> i32 {
    STATE.with_borrow_mut(|state| state.runtime.request_shutdown().map_or(-1, |()| 0))
}

#[must_use]
pub extern "C" fn riscbox_request_reboot() -> i32 {
    STATE.with_borrow_mut(|state| state.runtime.request_reboot().map_or(-1, |()| 0))
}

#[must_use]
pub extern "C" fn riscbox_console_input(address: u32, length: u32) -> u32 {
    STATE.with_borrow_mut(|state| {
        let Some(bytes) = allocated_bytes(state, address, length).map(<[u8]>::to_vec) else {
            return 0;
        };
        u32::try_from(state.input_queue.queue_console(&bytes)).unwrap_or(0)
    })
}

#[must_use]
pub extern "C" fn riscbox_console_resize(columns: u32, rows: u32) -> i32 {
    let (Ok(columns), Ok(rows)) = (u16::try_from(columns), u16::try_from(rows)) else {
        return -1;
    };
    STATE.with_borrow_mut(|state| state.input_queue.resize(columns, rows));
    0
}

#[must_use]
pub extern "C" fn riscbox_key_event(pressed: u32, code: u32) -> i32 {
    let Ok(code) = u16::try_from(code) else {
        return -1;
    };
    STATE.with_borrow_mut(|state| state.input_queue.key_event(pressed != 0, code));
    0
}

#[must_use]
pub extern "C" fn riscbox_pointer_event(x: u32, y: u32, buttons: u32) -> i32 {
    STATE.with_borrow_mut(|state| state.input_queue.pointer_event(x, y, buttons));
    0
}

#[must_use]
pub extern "C" fn riscbox_wheel_event(delta: i32) -> i32 {
    STATE.with_borrow_mut(|state| state.input_queue.wheel_event(delta));
    0
}

#[must_use]
pub extern "C" fn riscbox_network_input(address: u32, length: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        let Some(bytes) = allocated_bytes(state, address, length).map(<[u8]>::to_vec) else {
            return -1;
        };
        match state.input_queue.network_packet(&bytes) {
            NetworkInputResult::Accepted => 0,
            NetworkInputResult::Dropped => 1,
        }
    })
}

#[must_use]
pub extern "C" fn riscbox_network_carrier(up: u32) -> i32 {
    STATE.with_borrow_mut(|state| state.input_queue.network_carrier(up != 0));
    0
}

fn u64_from_parts(low: u32, high: u32) -> u64 {
    u64::from(low) | (u64::from(high) << 32)
}

#[must_use]
pub extern "C" fn riscbox_configure_quantum(target_quantum_ms: f64, diagnostics: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        state
            .runtime
            .configure_quantum(target_quantum_ms, diagnostics != 0)
            .map_or(-1, |()| 0)
    })
}

#[must_use]
pub extern "C" fn riscbox_wake_delay_ms(
    host_epoch_ms_low: u32,
    host_epoch_ms_high: u32,
    requested: u32,
) -> u32 {
    STATE.with_borrow(|state| {
        state.runtime.wake_delay_ms(
            u64_from_parts(host_epoch_ms_low, host_epoch_ms_high),
            requested,
        )
    })
}

#[must_use]
pub extern "C" fn riscbox_quantum_begin(host_epoch_ms_low: u32, host_epoch_ms_high: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        match state
            .runtime
            .begin_quantum(u64_from_parts(host_epoch_ms_low, host_epoch_ms_high))
        {
            QuantumStart::Ready => 0,
            QuantumStart::VmInactive => -1,
            QuantumStart::AlreadyActive => -2,
        }
    })
}

#[must_use]
pub extern "C" fn riscbox_quantum_run() -> i32 {
    STATE.with_borrow_mut(|state| {
        let AbiState {
            runtime,
            input_queue,
            ..
        } = state;
        match runtime.run_quantum(input_queue) {
            Ok(QuantumOutcome::BudgetReached) => 0,
            Ok(QuantumOutcome::WfiSleep) => 1,
            Ok(QuantumOutcome::HostServiceRequired) => 2,
            Ok(QuantumOutcome::VmInactive) => 3,
            Err(_) => -1,
        }
    })
}

#[must_use]
pub extern "C" fn riscbox_quantum_finish(
    host_elapsed_ms: f64,
    host_epoch_ms_low: u32,
    host_epoch_ms_high: u32,
) -> i32 {
    STATE.with_borrow_mut(|state| {
        state
            .runtime
            .finish_quantum(
                host_elapsed_ms,
                u64_from_parts(host_epoch_ms_low, host_epoch_ms_high),
            )
            .map_or(-1, |delay| i32::try_from(delay).unwrap_or(i32::MAX))
    })
}

pub extern "C" fn riscbox_quantum_abort() {
    STATE.with_borrow_mut(|state| state.runtime.abort_quantum());
}

#[must_use]
pub extern "C" fn riscbox_timing_stat(kind: u32) -> f64 {
    STATE.with_borrow(|state| state.runtime.timing_stat(kind))
}

#[must_use]
pub extern "C" fn riscbox_next_action() -> u32 {
    STATE.with_borrow_mut(|state| {
        state.action = state.runtime.next_action();
        match state.action {
            Some(HostAction::Request(_)) => 1,
            Some(HostAction::Started) => 2,
            Some(HostAction::Prepared) => 15,
            Some(HostAction::Console(_)) => 3,
            Some(HostAction::Network(_)) => 4,
            Some(HostAction::Framebuffer(_)) => 6,
            Some(HostAction::Halted(_)) => 10,
            Some(HostAction::Reset(_)) => 11,
            None => 0,
        }
    })
}

#[must_use]
pub extern "C" fn riscbox_action_value() -> u32 {
    STATE.with_borrow(|state| match state.action.as_ref() {
        Some(HostAction::Request(request)) => request.id,
        Some(HostAction::Halted(cause) | HostAction::Reset(cause)) => *cause as u32,
        _ => 0,
    })
}

#[must_use]
pub extern "C" fn riscbox_action_disk() -> u32 {
    STATE.with_borrow(|state| match &state.action {
        Some(HostAction::Request(request)) => request.disk.map_or(0, |disk| disk.0 + 1),
        _ => 0,
    })
}

#[must_use]
pub extern "C" fn riscbox_action_data_address() -> u32 {
    STATE.with_borrow(|state| {
        action_bytes(state)
            .and_then(|bytes| u32::try_from(bytes.as_ptr() as usize).ok())
            .unwrap_or(0)
    })
}

#[must_use]
pub extern "C" fn riscbox_action_data_length() -> u32 {
    STATE.with_borrow(|state| {
        action_bytes(state)
            .and_then(|bytes| u32::try_from(bytes.len()).ok())
            .unwrap_or(0)
    })
}

#[must_use]
pub extern "C" fn riscbox_action_x() -> u32 {
    framebuffer_action(|update| update.x)
}

#[must_use]
pub extern "C" fn riscbox_action_y() -> u32 {
    framebuffer_action(|update| update.y)
}

#[must_use]
pub extern "C" fn riscbox_action_width() -> u32 {
    framebuffer_action(|update| update.width)
}

#[must_use]
pub extern "C" fn riscbox_action_height() -> u32 {
    framebuffer_action(|update| update.height)
}

#[must_use]
pub extern "C" fn riscbox_action_stride() -> u32 {
    framebuffer_action(|update| update.stride)
}

#[must_use]
pub extern "C" fn riscbox_http_complete(id: u32, status: u32, address: u32, length: u32) -> i32 {
    let Ok(status) = u16::try_from(status) else {
        return -1;
    };
    STATE.with_borrow_mut(|state| {
        let Some(bytes) = completion_bytes(state, address, length) else {
            return -1;
        };
        state
            .runtime
            .complete_http(id, status, bytes)
            .map_or(-1, |()| 0)
    })
}

#[must_use]
pub fn take_start_request() -> Option<StartRequest> {
    STATE.with_borrow_mut(|state| state.start.take())
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

fn completion_bytes(state: &AbiState, address: u32, length: u32) -> Option<Vec<u8>> {
    if length == 0 {
        Some(Vec::new())
    } else {
        allocated_bytes(state, address, length).map(<[u8]>::to_vec)
    }
}

fn action_bytes(state: &AbiState) -> Option<&[u8]> {
    match state.action.as_ref()? {
        HostAction::Request(request) => Some(request.url.as_bytes()),
        HostAction::Console(bytes) | HostAction::Network(bytes) => Some(bytes),
        HostAction::Framebuffer(update) => state.runtime.framebuffer_bytes(*update),
        HostAction::Prepared
        | HostAction::Started
        | HostAction::Halted(_)
        | HostAction::Reset(_) => None,
    }
}

fn framebuffer_action(value: impl FnOnce(&crate::machine::FramebufferUpdate) -> u32) -> u32 {
    STATE.with_borrow(|state| match state.action.as_ref() {
        Some(HostAction::Framebuffer(update)) => value(update),
        _ => 0,
    })
}

#[cfg(test)]
mod tests {
    use super::u64_from_parts;

    #[test]
    fn reconstructs_epoch_ticks_without_truncation() {
        assert_eq!(
            u64_from_parts(0x2345_6789, 0x0123_4567),
            0x0123_4567_2345_6789,
        );
    }
}
