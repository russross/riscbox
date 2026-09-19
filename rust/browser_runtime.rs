//! Browser startup and execution state independent of the JavaScript adapter.

use core::fmt;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use crate::browser::{BrowserController, BrowserEvent, RunPolicy};
use crate::browser_storage::HttpBlockStore;
use crate::config::{Console, FilesystemBackend, VmConfig, resolve_asset_path};
use crate::entropy::{EntropyError, EntropySource, SharedEntropy};
use crate::machine::{
    BootImages, FramebufferConfig, FramebufferUpdate, Machine, MachineConfig, MachineError,
};
use crate::virtio_devices::{DeviceError, InputKind, NetworkBackend, NinePBackend};

pub type NinePCallback = Rc<RefCell<dyn FnMut(&[u8]) -> Result<Vec<u8>, DeviceError>>>;
pub type EntropyCallback = Rc<RefCell<dyn FnMut(&mut [u8]) -> Result<(), EntropyError>>>;

struct CallbackEntropy {
    callback: EntropyCallback,
}

impl EntropySource for CallbackEntropy {
    fn fill(&mut self, destination: &mut [u8]) -> Result<(), EntropyError> {
        (self.callback.borrow_mut())(destination)
    }
}

pub struct CallbackNineP {
    callback: NinePCallback,
}

impl CallbackNineP {
    #[must_use]
    pub fn new(callback: NinePCallback) -> Self {
        Self { callback }
    }
}

impl NinePBackend for CallbackNineP {
    fn transact(&mut self, request: &[u8]) -> Result<Vec<u8>, DeviceError> {
        (self.callback.borrow_mut())(request)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeStart {
    pub config_url: String,
    pub ram_mib: u32,
    pub command_line: String,
    pub width: u32,
    pub height: u32,
    pub has_network: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpRequest {
    pub id: u32,
    pub url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostAction {
    Request(HttpRequest),
    Started,
    Console(Vec<u8>),
    Network(Vec<u8>),
    Framebuffer(FramebufferUpdate),
    Schedule(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeError {
    AlreadyStarted,
    UnexpectedResponse(u32),
    HttpStatus(u16),
    InvalidConfig(String),
    MissingFirmware,
    Unsupported(&'static str),
    Machine(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "browser runtime error: {self:?}")
    }
}

impl std::error::Error for RuntimeError {}

impl From<MachineError> for RuntimeError {
    fn from(error: MachineError) -> Self {
        Self::Machine(error.to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AssetKind {
    Firmware,
    Kernel,
    Initrd,
    Drive(usize),
}

struct Loading {
    start: RuntimeStart,
    config: Option<VmConfig>,
    assets: VecDeque<(AssetKind, String)>,
    waiting: Option<(u32, AssetKind)>,
    firmware: Option<Vec<u8>>,
    kernel: Option<Vec<u8>>,
    initrd: Option<Vec<u8>>,
    drive_manifests: Vec<Option<(String, Vec<u8>)>>,
}

struct Running {
    machine: Machine,
    console_slot: Option<usize>,
    uart_output: bool,
    network_slot: Option<usize>,
    keyboard_slot: Option<usize>,
    pointer_slot: Option<usize>,
    network_output: Rc<RefCell<VecDeque<Vec<u8>>>>,
    block_slots: Vec<usize>,
    pending_http: BTreeMap<u32, (usize, u32)>,
}

enum State {
    Idle,
    Config {
        start: RuntimeStart,
        request_id: u32,
    },
    Loading(Box<Loading>),
    Running(Box<Running>),
}

pub struct BrowserRuntime {
    state: State,
    next_request_id: u32,
    actions: VecDeque<HostAction>,
    policy: RunPolicy,
    ninep: Option<NinePCallback>,
    entropy: Option<EntropyCallback>,
}

impl Default for BrowserRuntime {
    fn default() -> Self {
        Self {
            state: State::Idle,
            next_request_id: 1,
            actions: VecDeque::new(),
            policy: RunPolicy::default(),
            ninep: None,
            entropy: None,
        }
    }
}

impl BrowserRuntime {
    pub fn set_ninep_callback(&mut self, callback: NinePCallback) {
        self.ninep = Some(callback);
    }

    pub fn set_entropy_callback(&mut self, callback: EntropyCallback) {
        self.entropy = Some(callback);
    }

    fn entropy_source(&self) -> Result<SharedEntropy, RuntimeError> {
        if let Some(callback) = &self.entropy {
            Ok(Rc::new(RefCell::new(CallbackEntropy {
                callback: callback.clone(),
            })))
        } else {
            Ok(Rc::new(RefCell::new(
                crate::entropy::SystemEntropy::open()
                    .map_err(|error| RuntimeError::Machine(error.to_string()))?,
            )))
        }
    }

    fn create_machine(
        &self,
        ram_size: u64,
        framebuffer: Option<FramebufferConfig>,
    ) -> Result<Machine, RuntimeError> {
        Ok(Machine::new_with_entropy(
            MachineConfig {
                ram_size,
                framebuffer,
            },
            self.entropy_source()?,
        )?)
    }
    /// Begins loading a VM configuration.
    ///
    /// # Errors
    ///
    /// Returns `AlreadyStarted` unless the runtime is idle.
    pub fn start(&mut self, start: RuntimeStart) -> Result<(), RuntimeError> {
        if !matches!(self.state, State::Idle) {
            return Err(RuntimeError::AlreadyStarted);
        }
        let request_id = self.allocate_request(&start.config_url);
        self.state = State::Config { start, request_id };
        Ok(())
    }

    /// Supplies the response for the currently pending HTTP request.
    ///
    /// # Errors
    ///
    /// Returns an error for an unexpected request, failed HTTP status, malformed
    /// configuration, unsupported browser device, or invalid machine layout.
    pub fn complete_http(
        &mut self,
        id: u32,
        status: u16,
        bytes: Vec<u8>,
    ) -> Result<(), RuntimeError> {
        if !(200..300).contains(&status) {
            return Err(RuntimeError::HttpStatus(status));
        }
        let state = core::mem::replace(&mut self.state, State::Idle);
        match state {
            State::Config { start, request_id } if request_id == id => {
                let source = String::from_utf8(bytes).map_err(|_| {
                    RuntimeError::InvalidConfig("configuration is not UTF-8".into())
                })?;
                let mut config = VmConfig::parse(&source)
                    .map_err(|error| RuntimeError::InvalidConfig(error.to_string()))?;
                if !start.command_line.is_empty() {
                    config.apply_command_line(&start.command_line);
                }
                let mut assets = VecDeque::new();
                for (kind, path) in [
                    (AssetKind::Firmware, config.bios.as_deref()),
                    (AssetKind::Kernel, config.kernel.as_deref()),
                    (AssetKind::Initrd, config.initrd.as_deref()),
                ] {
                    if let Some(path) = path {
                        assets.push_back((kind, resolve_asset_path(Some(&start.config_url), path)));
                    }
                }
                for (index, drive) in config.drives.iter().enumerate() {
                    assets.push_back((
                        AssetKind::Drive(index),
                        resolve_asset_path(Some(&start.config_url), &drive.file),
                    ));
                }
                let drive_count = config.drives.len();
                let loading = Loading {
                    start,
                    config: Some(config),
                    assets,
                    waiting: None,
                    firmware: None,
                    kernel: None,
                    initrd: None,
                    drive_manifests: (0..drive_count).map(|_| None).collect(),
                };
                self.state = State::Loading(Box::new(loading));
                self.request_next_asset()
            }
            State::Loading(mut loading) => {
                let Some((request_id, kind)) = loading.waiting.take() else {
                    self.state = State::Loading(loading);
                    return Err(RuntimeError::UnexpectedResponse(id));
                };
                if request_id != id {
                    loading.waiting = Some((request_id, kind));
                    self.state = State::Loading(loading);
                    return Err(RuntimeError::UnexpectedResponse(id));
                }
                match kind {
                    AssetKind::Firmware => loading.firmware = Some(bytes),
                    AssetKind::Kernel => loading.kernel = Some(bytes),
                    AssetKind::Initrd => loading.initrd = Some(bytes),
                    AssetKind::Drive(index) => {
                        let url = config_asset_url(&loading, index)?;
                        loading.drive_manifests[index] = Some((url, bytes));
                    }
                }
                self.state = State::Loading(loading);
                self.request_next_asset()
            }
            State::Running(mut running) => {
                let Some((slot, request)) = running.pending_http.remove(&id) else {
                    self.state = State::Running(running);
                    return Err(RuntimeError::UnexpectedResponse(id));
                };
                running
                    .machine
                    .complete_http_block_request(slot, request, bytes)?;
                self.state = State::Running(running);
                self.pump_http_requests()
            }
            other => {
                self.state = other;
                Err(RuntimeError::UnexpectedResponse(id))
            }
        }
    }

    /// Runs one bounded interpreter slice and collects host-facing output.
    ///
    /// # Errors
    ///
    /// Returns an error when queued input exposes an invalid guest device queue.
    pub fn run(
        &mut self,
        controller: &mut BrowserController,
        timer_ticks: u64,
        host_nanoseconds: u64,
    ) -> Result<(), RuntimeError> {
        let State::Running(running) = &mut self.state else {
            return Ok(());
        };
        if let Some(size) = controller.take_resize()
            && let Some(slot) = running.console_slot
        {
            running
                .machine
                .resize_virtio_console(slot, size.columns, size.rows)?;
        }
        let mut input = [0; 128];
        let count = controller.read_console(&mut input);
        if count != 0 {
            if let Some(slot) = running.console_slot {
                running
                    .machine
                    .virtio_console_receive(slot, &input[..count])?;
            } else {
                running.machine.receive_console(&input[..count]);
            }
        }
        while let Some(event) = controller.next_event() {
            match event {
                BrowserEvent::Key(event) => {
                    if let Some(slot) = running.keyboard_slot {
                        running
                            .machine
                            .virtio_key_event(slot, event.code, event.pressed)?;
                    }
                }
                BrowserEvent::Pointer(event) => {
                    if let Some(slot) = running.pointer_slot {
                        running.machine.virtio_pointer_event(
                            slot,
                            (
                                i32::try_from(event.x).unwrap_or(i32::MAX),
                                i32::try_from(event.y).unwrap_or(i32::MAX),
                            ),
                            event.wheel,
                            event.buttons,
                        )?;
                    }
                }
                BrowserEvent::NetworkPacket(packet) => {
                    if let Some(slot) = running.network_slot {
                        running.machine.virtio_network_receive(slot, packet)?;
                    }
                }
                BrowserEvent::NetworkCarrier(_) => {}
            }
        }
        running.machine.update_time(timer_ticks, host_nanoseconds);
        for _ in 0..self.policy.blocks_per_slice() {
            let outcome = running.machine.run(self.policy.block_cycles);
            if outcome.state == crate::cpu::RunState::Waiting {
                break;
            }
        }
        let uart = running.machine.take_console_output();
        let mut console = if running.uart_output {
            uart
        } else {
            Vec::new()
        };
        if let Some(slot) = running.console_slot {
            console.extend(running.machine.take_virtio_console_output(slot)?);
        }
        if !console.is_empty() {
            self.actions.push_back(HostAction::Console(console));
        }
        self.actions.extend(
            running
                .network_output
                .borrow_mut()
                .drain(..)
                .map(HostAction::Network),
        );
        for span in running.machine.take_redraw_spans()? {
            if let Some(update) = running.machine.framebuffer_update(span)? {
                self.actions.push_back(HostAction::Framebuffer(update));
            }
        }
        self.actions.push_back(HostAction::Schedule(
            self.policy.scheduled_delay(self.policy.maximum_delay_ms),
        ));
        self.pump_http_requests()
    }

    pub fn next_action(&mut self) -> Option<HostAction> {
        self.actions.pop_front()
    }

    #[must_use]
    pub fn framebuffer_bytes(&self, update: FramebufferUpdate) -> Option<&[u8]> {
        let State::Running(running) = &self.state else {
            return None;
        };
        running.machine.framebuffer_bytes(update)
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self.state, State::Running(_))
    }

    fn allocate_request(&mut self, url: &str) -> u32 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.actions.push_back(HostAction::Request(HttpRequest {
            id,
            url: url.to_owned(),
        }));
        id
    }

    fn request_next_asset(&mut self) -> Result<(), RuntimeError> {
        let State::Loading(mut loading) = core::mem::replace(&mut self.state, State::Idle) else {
            unreachable!();
        };
        if let Some((kind, url)) = loading.assets.pop_front() {
            let id = self.allocate_request(&url);
            loading.waiting = Some((id, kind));
            self.state = State::Loading(loading);
            return Ok(());
        }
        self.finish_loading(*loading)
    }

    fn finish_loading(&mut self, mut loading: Loading) -> Result<(), RuntimeError> {
        let config = loading.config.take().expect("loading state retains config");
        if config
            .filesystems
            .iter()
            .any(|filesystem| filesystem.backend != FilesystemBackend::JavaScript9p)
        {
            return Err(RuntimeError::Unsupported("file and socket 9p filesystems"));
        }
        if !config.filesystems.is_empty() && self.ninep.is_none() {
            return Err(RuntimeError::Unsupported("JavaScript 9p server"));
        }
        let firmware = loading
            .firmware
            .as_deref()
            .ok_or(RuntimeError::MissingFirmware)?;
        let ram_size = u64::from(loading.start.ram_mib)
            .checked_shl(20)
            .ok_or_else(|| RuntimeError::Machine("RAM size overflow".into()))?;
        let dimensions = if loading.start.width != 0 && loading.start.height != 0 {
            Some((loading.start.width, loading.start.height))
        } else {
            config.display.as_ref().map(|display| {
                (
                    u32::try_from(display.width).unwrap_or(0),
                    u32::try_from(display.height).unwrap_or(0),
                )
            })
        };
        let framebuffer = dimensions
            .filter(|(width, height)| *width != 0 && *height != 0)
            .map(|(width, height)| FramebufferConfig { width, height });
        let mut machine = self.create_machine(ram_size, framebuffer)?;
        let mut block_slots = Vec::new();
        for (index, manifest) in loading.drive_manifests.into_iter().enumerate() {
            let (url, bytes) = manifest.expect("each drive manifest is loaded in order");
            let source = String::from_utf8(bytes)
                .map_err(|_| RuntimeError::InvalidConfig("drive manifest is not UTF-8".into()))?;
            let store = HttpBlockStore::from_manifest(&url, &source, 16 << 20)
                .map_err(|error| RuntimeError::Machine(error.to_string()))?;
            let mut id = [0; 20];
            let name = format!("riscbox-http-{index}");
            id[..name.len()].copy_from_slice(name.as_bytes());
            block_slots.push(machine.add_http_block_device(store, id)?);
        }
        for filesystem in &config.filesystems {
            let callback = self
                .ninep
                .as_ref()
                .expect("JavaScript 9p callback was validated")
                .clone();
            machine.add_ninep_device(
                Box::new(CallbackNineP::new(callback)),
                filesystem.tag.as_bytes(),
            )?;
        }
        let console_slot = if config.console == Console::Virtio {
            Some(machine.add_console_device(80, 25)?)
        } else {
            None
        };
        let network_output = Rc::new(RefCell::new(VecDeque::new()));
        let network_slot = if loading.start.has_network && !config.networks.is_empty() {
            Some(machine.add_network_device(
                Box::new(OutputNetwork(network_output.clone())),
                [0x02, 0, 0, 0, 0, 1],
            )?)
        } else {
            None
        };
        let (keyboard_slot, pointer_slot) = if config.input_device.as_deref() == Some("virtio") {
            (
                Some(machine.add_input_device(InputKind::Keyboard)?),
                Some(machine.add_input_device(InputKind::Tablet)?),
            )
        } else {
            (None, None)
        };
        machine.add_entropy_device()?;
        machine.load_boot(BootImages {
            firmware,
            kernel: loading.kernel.as_deref(),
            initrd: loading.initrd.as_deref(),
            command_line: config.command_line.as_deref().unwrap_or_default(),
        })?;
        self.state = State::Running(Box::new(Running {
            machine,
            console_slot,
            uart_output: config.console == Console::Uart || config.uart_output,
            network_slot,
            keyboard_slot,
            pointer_slot,
            network_output,
            block_slots,
            pending_http: BTreeMap::new(),
        }));
        self.actions.push_back(HostAction::Started);
        self.actions.push_back(HostAction::Schedule(0));
        self.pump_http_requests()
    }

    fn pump_http_requests(&mut self) -> Result<(), RuntimeError> {
        let State::Running(mut running) = core::mem::replace(&mut self.state, State::Idle) else {
            return Ok(());
        };
        for slot in running.block_slots.clone() {
            while let Some(request) = running.machine.next_http_block_request(slot)? {
                let id = self.allocate_request(&request.url);
                running.pending_http.insert(id, (slot, request.id));
            }
        }
        self.state = State::Running(running);
        Ok(())
    }
}

fn config_asset_url(loading: &Loading, index: usize) -> Result<String, RuntimeError> {
    let config = loading
        .config
        .as_ref()
        .expect("loading state retains config");
    let drive = config
        .drives
        .get(index)
        .ok_or_else(|| RuntimeError::InvalidConfig("invalid drive index".into()))?;
    Ok(resolve_asset_path(
        Some(&loading.start.config_url),
        &drive.file,
    ))
}

struct OutputNetwork(Rc<RefCell<VecDeque<Vec<u8>>>>);

impl NetworkBackend for OutputNetwork {
    fn transmit(&mut self, packet: &[u8]) -> Result<(), DeviceError> {
        self.0.borrow_mut().push_back(packet.to_vec());
        Ok(())
    }
}
