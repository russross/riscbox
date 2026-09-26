//! Host-neutral state at the raw WebAssembly browser boundary.

use std::collections::VecDeque;

use crate::virtio_devices::{
    MAX_NETWORK_FRAME_SIZE, MAX_PENDING_NETWORK_BYTES, MAX_PENDING_NETWORK_FRAMES,
};

pub const CONSOLE_INPUT_CAPACITY: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkInputResult {
    Accepted,
    Dropped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    pub pressed: bool,
    pub code: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PointerEvent {
    pub x: u32,
    pub y: u32,
    pub wheel: i32,
    pub buttons: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserEvent {
    Key(KeyEvent),
    Pointer(PointerEvent),
    NetworkPacket(Vec<u8>),
    NetworkCarrier(bool),
}

#[derive(Debug)]
pub struct BrowserController {
    console: VecDeque<u8>,
    resize: Option<TerminalSize>,
    events: VecDeque<BrowserEvent>,
    pointer_x: u32,
    pointer_y: u32,
    pointer_buttons: u32,
    network_carrier: bool,
    network_frames: usize,
    network_bytes: usize,
}

impl Default for BrowserController {
    fn default() -> Self {
        Self {
            console: VecDeque::with_capacity(CONSOLE_INPUT_CAPACITY),
            resize: None,
            events: VecDeque::new(),
            pointer_x: 0,
            pointer_y: 0,
            pointer_buttons: 0,
            network_carrier: false,
            network_frames: 0,
            network_bytes: 0,
        }
    }
}

impl BrowserController {
    /// Queues as much terminal input as the fixed browser FIFO can hold.
    pub fn queue_console(&mut self, data: &[u8]) -> usize {
        let accepted = data
            .len()
            .min(CONSOLE_INPUT_CAPACITY.saturating_sub(self.console.len()));
        self.console.extend(&data[..accepted]);
        accepted
    }

    /// Removes queued terminal bytes into `output`.
    pub fn read_console(&mut self, output: &mut [u8]) -> usize {
        let count = output.len().min(self.console.len());
        for (byte, queued) in output[..count].iter_mut().zip(self.console.drain(..count)) {
            *byte = queued;
        }
        count
    }

    #[must_use]
    pub fn console_len(&self) -> usize {
        self.console.len()
    }

    /// Records the latest terminal dimensions.
    pub fn resize(&mut self, columns: u16, rows: u16) {
        self.resize = Some(TerminalSize { columns, rows });
    }

    /// Takes a pending terminal resize.
    pub fn take_resize(&mut self) -> Option<TerminalSize> {
        self.resize.take()
    }

    /// Queues a keyboard transition.
    pub fn key_event(&mut self, pressed: bool, code: u16) {
        self.events
            .push_back(BrowserEvent::Key(KeyEvent { pressed, code }));
    }

    /// Queues a pointer position and button state.
    pub fn pointer_event(&mut self, x: u32, y: u32, buttons: u32) {
        self.pointer_x = x;
        self.pointer_y = y;
        self.pointer_buttons = buttons;
        self.events.push_back(BrowserEvent::Pointer(PointerEvent {
            x,
            y,
            wheel: 0,
            buttons,
        }));
    }

    /// Queues a wheel movement at the last pointer position.
    pub fn wheel_event(&mut self, wheel: i32) {
        self.events.push_back(BrowserEvent::Pointer(PointerEvent {
            x: self.pointer_x,
            y: self.pointer_y,
            wheel,
            buttons: self.pointer_buttons,
        }));
    }

    /// Queues one packet received from the browser network backend.
    pub fn network_packet(&mut self, packet: &[u8]) -> NetworkInputResult {
        let within_byte_limit = self
            .network_bytes
            .checked_add(packet.len())
            .is_some_and(|total| total <= MAX_PENDING_NETWORK_BYTES);
        if packet.is_empty()
            || packet.len() > MAX_NETWORK_FRAME_SIZE
            || self.network_frames >= MAX_PENDING_NETWORK_FRAMES
            || !within_byte_limit
        {
            return NetworkInputResult::Dropped;
        }
        self.events
            .push_back(BrowserEvent::NetworkPacket(packet.to_vec()));
        self.network_frames += 1;
        self.network_bytes += packet.len();
        NetworkInputResult::Accepted
    }

    /// Records and queues a browser network carrier transition.
    pub fn network_carrier(&mut self, up: bool) {
        self.network_carrier = up;
        self.events.push_back(BrowserEvent::NetworkCarrier(up));
    }

    #[must_use]
    pub const fn carrier_is_up(&self) -> bool {
        self.network_carrier
    }

    /// Removes the oldest pending browser event.
    pub fn next_event(&mut self) -> Option<BrowserEvent> {
        let event = self.events.pop_front()?;
        if let BrowserEvent::NetworkPacket(packet) = &event {
            self.network_frames -= 1;
            self.network_bytes -= packet.len();
        }
        Some(event)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunPolicy {
    pub yield_cycles: u32,
    pub maximum_delay_ms: u32,
}

impl Default for RunPolicy {
    fn default() -> Self {
        Self {
            yield_cycles: i32::MAX as u32,
            maximum_delay_ms: 100,
        }
    }
}
