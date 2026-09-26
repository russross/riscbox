//! Safe storage and exported scalar calls for the raw WebAssembly adapter.

use std::cell::RefCell;

use crate::browser::{BrowserController, NetworkInputResult};
use crate::browser_runtime::{
    BrowserRuntime, EntropyCallback, HostAction, RuntimeStart, TurnExit, TurnStart,
};
use crate::virtio_devices::{
    NinePEndpointId, NinePGeneration, NinePOutcome, NinePRequestId, NinePTransportAction,
};

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
    controller: BrowserController,
    start: Option<StartRequest>,
    runtime: BrowserRuntime,
    action: Option<HostAction>,
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
        match state.controller.network_packet(&bytes) {
            NetworkInputResult::Accepted => 0,
            NetworkInputResult::Dropped => 1,
        }
    })
}

#[must_use]
pub extern "C" fn riscbox_network_carrier(up: u32) -> i32 {
    STATE.with_borrow_mut(|state| state.controller.network_carrier(up != 0));
    0
}

fn u64_from_parts(low: u32, high: u32) -> u64 {
    u64::from(low) | (u64::from(high) << 32)
}

#[must_use]
pub extern "C" fn riscbox_configure_timing(timeslice_ms: f64, diagnostics: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        state
            .runtime
            .configure_timing(timeslice_ms, diagnostics != 0)
            .map_or(-1, |()| 0)
    })
}

#[must_use]
pub extern "C" fn riscbox_wake_delay_ms(now_low: u32, now_high: u32, requested: u32) -> u32 {
    STATE.with_borrow(|state| {
        state
            .runtime
            .wake_delay_ms(u64_from_parts(now_low, now_high), requested)
    })
}

#[must_use]
pub extern "C" fn riscbox_turn_begin(now_low: u32, now_high: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        match state.runtime.begin_turn(u64_from_parts(now_low, now_high)) {
            TurnStart::Ready => 0,
            TurnStart::Delay(delay) => i32::try_from(delay).unwrap_or(i32::MAX),
            TurnStart::Idle => -1,
        }
    })
}

#[must_use]
pub extern "C" fn riscbox_turn_advance() -> i32 {
    STATE.with_borrow_mut(|state| {
        let AbiState {
            runtime,
            controller,
            ..
        } = state;
        match runtime.advance_turn(controller) {
            Ok(TurnExit::Finished) => 0,
            Ok(TurnExit::Waiting) => 1,
            Ok(TurnExit::HostActions) => 2,
            Ok(TurnExit::Idle) => 3,
            Err(_) => -1,
        }
    })
}

#[must_use]
pub extern "C" fn riscbox_turn_finish(elapsed_ms: f64, now_low: u32, now_high: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        state
            .runtime
            .finish_turn(elapsed_ms, u64_from_parts(now_low, now_high))
            .map_or(-1, |delay| i32::try_from(delay).unwrap_or(i32::MAX))
    })
}

pub extern "C" fn riscbox_turn_abort() {
    STATE.with_borrow_mut(|state| state.runtime.abort_turn());
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
            Some(HostAction::Console(_)) => 3,
            Some(HostAction::Network(_)) => 4,
            Some(HostAction::Framebuffer(_)) => 6,
            Some(HostAction::NineP(NinePTransportAction::Open { .. })) => 7,
            Some(HostAction::NineP(NinePTransportAction::Request { .. })) => 8,
            Some(HostAction::NineP(NinePTransportAction::Close { .. })) => 9,
            None => 0,
        }
    })
}

#[must_use]
pub extern "C" fn riscbox_action_value() -> u32 {
    STATE.with_borrow(|state| match state.action.as_ref() {
        Some(HostAction::Request(request)) => request.id,
        _ => 0,
    })
}

#[must_use]
pub extern "C" fn riscbox_action_endpoint() -> u32 {
    STATE.with_borrow(|state| match state.action.as_ref() {
        Some(HostAction::NineP(action)) => match action {
            NinePTransportAction::Open { endpoint, .. }
            | NinePTransportAction::Request { endpoint, .. }
            | NinePTransportAction::Close { endpoint, .. } => endpoint.0,
        },
        _ => 0,
    })
}

#[must_use]
pub extern "C" fn riscbox_action_generation() -> u32 {
    STATE.with_borrow(|state| match state.action.as_ref() {
        Some(HostAction::NineP(action)) => match action {
            NinePTransportAction::Open { generation, .. }
            | NinePTransportAction::Request { generation, .. }
            | NinePTransportAction::Close { generation, .. } => generation.0,
        },
        _ => 0,
    })
}

#[must_use]
pub extern "C" fn riscbox_action_request_id() -> u32 {
    STATE.with_borrow(|state| match state.action.as_ref() {
        Some(HostAction::NineP(NinePTransportAction::Request { request_id, .. })) => request_id.0,
        _ => 0,
    })
}

#[must_use]
pub extern "C" fn riscbox_action_reply_capacity() -> u32 {
    STATE.with_borrow(|state| match state.action.as_ref() {
        Some(HostAction::NineP(NinePTransportAction::Request { reply_capacity, .. })) => {
            *reply_capacity
        }
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
        let Some(bytes) = allocated_bytes(state, address, length).map(<[u8]>::to_vec) else {
            return -1;
        };
        state
            .runtime
            .complete_http(id, status, bytes)
            .map_or(-1, |()| 0)
    })
}

#[must_use]
pub extern "C" fn riscbox_p9_complete(
    endpoint: u32,
    generation: u32,
    request_id: u32,
    outcome: u32,
    address: u32,
    length: u32,
) -> i32 {
    STATE.with_borrow_mut(|state| {
        let outcome = match outcome {
            0 => {
                let Some(bytes) = completion_bytes(state, address, length) else {
                    return -1;
                };
                NinePOutcome::Reply(bytes)
            }
            1 if length == 0 => NinePOutcome::Suppressed,
            2 if length == 0 => NinePOutcome::EndpointFailure,
            _ => return -1,
        };
        state
            .runtime
            .complete_ninep(
                NinePEndpointId(endpoint),
                NinePGeneration(generation),
                NinePRequestId(request_id),
                outcome,
            )
            .map_or(-1, |()| 0)
    })
}

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
        HostAction::Console(bytes)
        | HostAction::Network(bytes)
        | HostAction::NineP(NinePTransportAction::Request { bytes, .. }) => Some(bytes),
        HostAction::Framebuffer(update) => state.runtime.framebuffer_bytes(*update),
        HostAction::NineP(NinePTransportAction::Open { server_key, .. }) => {
            Some(server_key.as_bytes())
        }
        HostAction::NineP(NinePTransportAction::Close { .. }) | HostAction::Started => None,
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
