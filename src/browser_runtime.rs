//! Browser startup and execution state independent of the JavaScript adapter.

use core::fmt;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use crate::browser::{BrowserController, BrowserEvent, RunPolicy};
use crate::browser_storage::HttpBlockStore;
use crate::config::{Console, VmConfig, resolve_asset_path};
use crate::entropy::{EntropyError, EntropySource, SharedEntropy};
use crate::machine::{
    BootImages, FramebufferConfig, FramebufferUpdate, Machine, MachineConfig, MachineError,
};
use crate::tinyemu_core::RunState;
use crate::virtio_devices::{
    DeviceError, InputKind, NetworkBackend, NinePBackend, NinePEndpointId, NinePGeneration,
    NinePOutcome, NinePRequestId, NinePTransportAction,
};

pub type EntropyCallback = Rc<RefCell<dyn FnMut(&mut [u8]) -> Result<(), EntropyError>>>;

struct CallbackEntropy {
    callback: EntropyCallback,
}

impl EntropySource for CallbackEntropy {
    fn fill(&mut self, destination: &mut [u8]) -> Result<(), EntropyError> {
        (self.callback.borrow_mut())(destination)
    }
}

pub struct BrowserNineP {
    endpoint: NinePEndpointId,
    generation: NinePGeneration,
    server_key: String,
    actions: VecDeque<NinePTransportAction>,
}

impl BrowserNineP {
    #[must_use]
    pub fn new(endpoint: NinePEndpointId, server_key: String) -> Self {
        let generation = NinePGeneration(1);
        let actions = VecDeque::from([NinePTransportAction::Open {
            endpoint,
            generation,
            server_key: server_key.clone(),
        }]);
        Self {
            endpoint,
            generation,
            server_key,
            actions,
        }
    }
}

impl NinePBackend for BrowserNineP {
    fn submit(&mut self, request_id: NinePRequestId, request: Vec<u8>, reply_capacity: u32) {
        self.actions.push_back(NinePTransportAction::Request {
            endpoint: self.endpoint,
            generation: self.generation,
            request_id,
            bytes: request,
            reply_capacity,
        });
    }

    fn reset(&mut self, generation: NinePGeneration) {
        self.actions.push_back(NinePTransportAction::Close {
            endpoint: self.endpoint,
            generation: self.generation,
        });
        self.generation = generation;
        self.actions.push_back(NinePTransportAction::Open {
            endpoint: self.endpoint,
            generation,
            server_key: self.server_key.clone(),
        });
    }

    fn next_transport_action(&mut self) -> Option<NinePTransportAction> {
        self.actions.pop_front()
    }

    fn has_transport_action(&self) -> bool {
        !self.actions.is_empty()
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
    NineP(NinePTransportAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrowserRunResult {
    pub cycles: u32,
    pub state: RunState,
    pub delay_ms: u32,
}

const TICKS_PER_SECOND: u128 = 10_000_000;
const TICKS_PER_MILLISECOND: u64 = 10_000;
const SAMPLE_HALFLIFE_MS: f64 = 5_000.0;
const INITIAL_CYCLES_PER_SECOND: f64 = 300_000_000.0;

fn integer_as_f64(value: u64) -> f64 {
    // Convert exact halves without an implicit precision-losing integer cast.
    let high = u32::try_from(value >> 32).expect("upper half fits");
    let low = u32::try_from(value & u64::from(u32::MAX)).expect("lower half fits");
    f64::from(high) * 4_294_967_296.0 + f64::from(low)
}

fn rounded_positive_integer(value: f64) -> u64 {
    // Turn rates and budgets are nonnegative. Decode the rounded binary float
    // so the integer boundary and saturation are explicit.
    let bits = value.round().to_bits();
    let exponent = i32::try_from((bits >> 52) & 0x7ff).expect("exponent fits") - 1023;
    if exponent < 0 {
        return 0;
    }
    if exponent >= 64 {
        return u64::MAX;
    }
    let mantissa = (bits & ((1_u64 << 52) - 1)) | (1_u64 << 52);
    if exponent >= 52 {
        mantissa.checked_shl(u32::try_from(exponent - 52).expect("shift fits"))
            .unwrap_or(u64::MAX)
    } else {
        mantissa >> u32::try_from(52 - exponent).expect("shift fits")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnStart {
    Ready,
    Delay(u32),
    Idle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnExit {
    HostActions,
    Finished,
    Waiting,
    Idle,
}

#[derive(Clone, Copy, Debug)]
struct RateEstimate {
    cycles: f64,
    milliseconds: f64,
    cycles_per_second: f64,
}

impl Default for RateEstimate {
    fn default() -> Self {
        Self {
            cycles: 0.0,
            milliseconds: 0.0,
            cycles_per_second: INITIAL_CYCLES_PER_SECOND,
        }
    }
}

impl RateEstimate {
    fn observe(&mut self, cycles: u64, milliseconds: f64) {
        if cycles == 0 || !milliseconds.is_finite() || milliseconds <= 0.0 {
            return;
        }
        let decay = 2.0_f64.powf(-milliseconds / SAMPLE_HALFLIFE_MS);
        // Decaying both totals weights each turn by its whole cycle count.
        self.cycles = self.cycles * decay + integer_as_f64(cycles);
        self.milliseconds = self.milliseconds * decay + milliseconds;
        self.cycles_per_second = self.cycles * 1_000.0 / self.milliseconds;
    }
}

#[derive(Debug)]
struct ActiveTurn {
    start_ticks: u64,
    rate: u64,
    budget: u32,
    used: u64,
    stalled_calls: u32,
    calls: u32,
    timer_exits: u32,
    timer_intervals: Vec<u64>,
    record_timer_interval: bool,
    idle_delay_ticks: u64,
    terminal: Option<TurnExit>,
}

impl ActiveTurn {
    fn ticks(&self) -> u64 {
        // One integer mapping owns guest time for the complete browser turn.
        let offset = u128::from(self.used) * TICKS_PER_SECOND / u128::from(self.rate);
        self.start_ticks
            .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX))
    }

    fn cycles_to_deadline(&self, delay_ticks: u64) -> u32 {
        // Ceiling inversion reaches the first cycle whose guest tick is due.
        let offset = u128::from(self.ticks() - self.start_ticks) + u128::from(delay_ticks);
        let numerator = offset
            .saturating_mul(u128::from(self.rate))
            .saturating_add(TICKS_PER_SECOND - 1);
        let target = numerator / TICKS_PER_SECOND;
        let additional = target.saturating_sub(u128::from(self.used)).max(1);
        u32::try_from(additional).unwrap_or(u32::MAX)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct TurnStatistics {
    calls: u32,
    timer_exits: u32,
    median_interval_ticks: u64,
    cycles: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeError {
    AlreadyStarted,
    UnexpectedResponse(u32),
    HttpStatus(u16),
    InvalidConfig(String),
    MissingFirmware,
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
    pointer_dimensions: Option<(u32, u32)>,
    network_output: Rc<RefCell<VecDeque<Vec<u8>>>>,
    block_slots: Vec<usize>,
    ninep_slots: Vec<usize>,
    ninep_endpoints: BTreeMap<NinePEndpointId, usize>,
    pending_http: BTreeMap<u32, PendingHttp>,
}

impl Running {
    fn deliver_event(&mut self, event: BrowserEvent) -> Result<(), MachineError> {
        match event {
            BrowserEvent::Key(event) => {
                if let Some(slot) = self.keyboard_slot {
                    self.machine
                        .virtio_key_event(slot, event.code, event.pressed)?;
                }
            }
            BrowserEvent::Pointer(event) => {
                if let Some(slot) = self.pointer_slot {
                    let position =
                        self.pointer_dimensions
                            .map_or((event.x, event.y), |(width, height)| {
                                (
                                    scale_pointer_coordinate(event.x, width),
                                    scale_pointer_coordinate(event.y, height),
                                )
                            });
                    self.machine.virtio_pointer_event(
                        slot,
                        (
                            i32::try_from(position.0).unwrap_or(i32::MAX),
                            i32::try_from(position.1).unwrap_or(i32::MAX),
                        ),
                        event.wheel,
                        event.buttons,
                    )?;
                }
            }
            BrowserEvent::NetworkPacket(packet) => {
                if let Some(slot) = self.network_slot {
                    self.machine.virtio_network_receive(slot, packet)?;
                }
            }
            BrowserEvent::NetworkCarrier(up) => {
                if let Some(slot) = self.network_slot {
                    self.machine.virtio_network_set_carrier(slot, up)?;
                }
            }
        }
        Ok(())
    }
}

enum PendingHttp {
    Block { slot: usize, request: u32 },
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
    entropy: Option<EntropyCallback>,
    timeslice_ms: f64,
    diagnostics: bool,
    rate: RateEstimate,
    guest_floor_ticks: u64,
    active_turn: Option<ActiveTurn>,
    last_turn: TurnStatistics,
}

impl Default for BrowserRuntime {
    fn default() -> Self {
        Self {
            state: State::Idle,
            next_request_id: 1,
            actions: VecDeque::new(),
            policy: RunPolicy::default(),
            entropy: None,
            timeslice_ms: 10.0,
            diagnostics: false,
            rate: RateEstimate::default(),
            guest_floor_ticks: 0,
            active_turn: None,
            last_turn: TurnStatistics::default(),
        }
    }
}

impl BrowserRuntime {
    /// Configures the duration and optional counters before a turn starts.
    ///
    /// # Errors
    /// Returns an error for an invalid duration or an active turn.
    pub fn configure_timing(
        &mut self,
        timeslice_ms: f64,
        diagnostics: bool,
    ) -> Result<(), RuntimeError> {
        if !timeslice_ms.is_finite()
            || timeslice_ms <= 0.0
            || timeslice_ms > 100.0
            || self.active_turn.is_some()
        {
            return Err(RuntimeError::InvalidConfig("invalid turn duration".into()));
        }
        self.timeslice_ms = timeslice_ms;
        self.diagnostics = diagnostics;
        Ok(())
    }

    #[must_use]
    pub fn wake_delay_ms(&self, wall_milliseconds: u64, requested_delay_ms: u32) -> u32 {
        let wall_ticks = wall_milliseconds.saturating_mul(TICKS_PER_MILLISECOND);
        let gap = self.guest_floor_ticks.saturating_sub(wall_ticks);
        let catchup = gap.div_ceil(TICKS_PER_MILLISECOND);
        requested_delay_ms.max(u32::try_from(catchup).unwrap_or(u32::MAX))
    }

    #[must_use]
    pub fn begin_turn(&mut self, wall_milliseconds: u64) -> TurnStart {
        if self.active_turn.is_some() {
            return TurnStart::Idle;
        }
        if !matches!(self.state, State::Running(_)) {
            return TurnStart::Idle;
        }
        let delay = self.wake_delay_ms(wall_milliseconds, 0);
        if delay != 0 {
            return TurnStart::Delay(delay);
        }
        let rate = rounded_positive_integer(self.rate.cycles_per_second.max(1.0));
        let budget = rounded_positive_integer((integer_as_f64(rate) * self.timeslice_ms / 1_000.0)
            .round()
            .clamp(1.0, f64::from(i32::MAX)));
        let budget = u32::try_from(budget).unwrap_or(i32::MAX as u32);
        self.active_turn = Some(ActiveTurn {
            start_ticks: wall_milliseconds
                .saturating_mul(TICKS_PER_MILLISECOND)
                .max(self.guest_floor_ticks),
            rate,
            budget,
            used: 0,
            stalled_calls: 0,
            calls: 0,
            timer_exits: 0,
            timer_intervals: Vec::new(),
            record_timer_interval: false,
            idle_delay_ticks: 0,
            terminal: None,
        });
        TurnStart::Ready
    }

    /// Runs until browser work or the turn boundary needs JavaScript.
    ///
    /// # Errors
    /// Returns an error for invalid guest I/O or an execution loop without progress.
    pub fn advance_turn(
        &mut self,
        controller: &mut BrowserController,
    ) -> Result<TurnExit, RuntimeError> {
        let Some(mut turn) = self.active_turn.take() else {
            return Err(RuntimeError::Machine("advance without an active turn".into()));
        };
        let result = self.advance_active_turn(controller, &mut turn);
        self.active_turn = Some(turn);
        result
    }

    fn advance_active_turn(
        &mut self,
        controller: &mut BrowserController,
        turn: &mut ActiveTurn,
    ) -> Result<TurnExit, RuntimeError> {
        if let Some(terminal) = turn.terminal {
            return Ok(terminal);
        }
        while turn.used < u64::from(turn.budget) {
            // Timer deadlines stay in guest ticks until the final CPU budget.
            let ticks = turn.ticks();
            let deadline = self.next_timer_delay_ticks(ticks);
            let remaining = u64::from(turn.budget) - turn.used;
            let mut call_budget = u32::try_from(remaining).unwrap_or(i32::MAX as u32);
            if let Some(delay_ticks) = deadline {
                call_budget = call_budget.min(turn.cycles_to_deadline(delay_ticks));
                if turn.record_timer_interval && self.diagnostics
                    && turn.timer_intervals.len() < 10_000
                {
                    turn.timer_intervals.push(delay_ticks);
                }
            }
            turn.record_timer_interval = false;
            let Some(outcome) = self.run(controller, ticks, ticks.saturating_mul(100), call_budget)?
            else {
                turn.terminal = Some(TurnExit::Idle);
                return Ok(TurnExit::Idle);
            };
            turn.used += u64::from(outcome.cycles);
            // The interpreter can overshoot a block boundary; actual cycles
            // determine both later guest time and the next calibration sample.
            if self.diagnostics {
                turn.calls += 1;
            }
            turn.stalled_calls = if outcome.cycles == 0 {
                turn.stalled_calls + 1
            } else {
                0
            };
            if turn.stalled_calls > 1 || outcome.state == RunState::Running && outcome.cycles == 0 {
                return Err(RuntimeError::Machine("CPU made no progress".into()));
            }
            if outcome.state == RunState::TimerChanged {
                turn.record_timer_interval = true;
                if self.diagnostics {
                    turn.timer_exits += 1;
                }
            }
            if outcome.state == RunState::Waiting {
                // Pending timers are already reflected in the machine state.
                // Only an actual WFI sleep leaves this turn waiting on the host.
                let next = self.next_timer_delay_ticks(turn.ticks());
                if let State::Running(running) = &self.state
                    && running.machine.is_waiting()
                {
                    turn.idle_delay_ticks = next.unwrap_or(u64::MAX).min(1_000_000);
                    turn.terminal = Some(TurnExit::Waiting);
                }
            }
            if turn.used >= u64::from(turn.budget) && turn.terminal.is_none() {
                turn.terminal = Some(TurnExit::Finished);
            }
            if !self.actions.is_empty() || outcome.state == RunState::HostAttention {
                return Ok(TurnExit::HostActions);
            }
            if let Some(terminal) = turn.terminal {
                return Ok(terminal);
            }
        }
        turn.terminal = Some(TurnExit::Finished);
        Ok(TurnExit::Finished)
    }

    /// Ends a completed turn and incorporates its elapsed host time.
    ///
    /// # Errors
    /// Returns an error when a turn is unfinished or elapsed time is invalid.
    pub fn finish_turn(
        &mut self,
        elapsed_ms: f64,
        wall_milliseconds: u64,
    ) -> Result<u32, RuntimeError> {
        if !elapsed_ms.is_finite() || elapsed_ms < 0.0 {
            return Err(RuntimeError::InvalidConfig("invalid turn elapsed time".into()));
        }
        let Some(turn) = self.active_turn.take() else {
            return Err(RuntimeError::Machine("finish without an active turn".into()));
        };
        if turn.terminal.is_none() {
            self.active_turn = Some(turn);
            return Err(RuntimeError::Machine("unfinished browser turn".into()));
        }
        self.guest_floor_ticks = turn.ticks();
        self.rate.observe(turn.used, elapsed_ms);
        if self.diagnostics {
            let mut intervals = turn.timer_intervals;
            intervals.sort_unstable();
            self.last_turn = TurnStatistics {
                calls: turn.calls,
                timer_exits: turn.timer_exits,
                median_interval_ticks: intervals.get(intervals.len() / 2).copied().unwrap_or(0),
                cycles: turn.used,
            };
        }
        let requested = if turn.terminal == Some(TurnExit::Waiting) {
            let due = self.guest_floor_ticks.saturating_add(turn.idle_delay_ticks);
            let wall = wall_milliseconds.saturating_mul(TICKS_PER_MILLISECOND);
            u32::try_from(due.saturating_sub(wall).div_ceil(TICKS_PER_MILLISECOND))
                .unwrap_or(u32::MAX)
        } else {
            0
        };
        Ok(requested)
    }

    pub fn abort_turn(&mut self) {
        if let Some(turn) = self.active_turn.take() {
            self.guest_floor_ticks = self.guest_floor_ticks.max(turn.ticks());
        }
    }

    #[must_use]
    pub fn timing_stat(&self, kind: u32) -> f64 {
        match kind {
            0 => self.rate.cycles_per_second,
            1 => f64::from(self.last_turn.calls),
            2 => f64::from(self.last_turn.timer_exits),
            3 => integer_as_f64(self.last_turn.median_interval_ticks),
            4 => integer_as_f64(self.last_turn.cycles),
            _ => 0.0,
        }
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
                let Some(pending) = running.pending_http.remove(&id) else {
                    self.state = State::Running(running);
                    return Err(RuntimeError::UnexpectedResponse(id));
                };
                match pending {
                    PendingHttp::Block { slot, request } => running
                        .machine
                        .complete_http_block_request(slot, request, bytes)?,
                }
                self.state = State::Running(running);
                self.pump_http_requests()
            }
            other => {
                self.state = other;
                Err(RuntimeError::UnexpectedResponse(id))
            }
        }
    }

    /// Completes one asynchronous browser 9p request.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown endpoint, current-generation duplicate,
    /// malformed response, endpoint failure, or invalid guest descriptor.
    pub fn complete_ninep(
        &mut self,
        endpoint: NinePEndpointId,
        generation: NinePGeneration,
        request_id: NinePRequestId,
        outcome: NinePOutcome,
    ) -> Result<(), RuntimeError> {
        let State::Running(mut running) = core::mem::replace(&mut self.state, State::Idle) else {
            return Err(RuntimeError::Machine(
                "9p completion without a running VM".into(),
            ));
        };
        let Some(slot) = running.ninep_endpoints.get(&endpoint).copied() else {
            self.state = State::Running(running);
            return Err(RuntimeError::Machine("unknown 9p endpoint".into()));
        };
        let endpoint_failure = matches!(outcome, NinePOutcome::EndpointFailure);
        let result = running
            .machine
            .complete_ninep_transport_request(slot, generation, request_id, outcome);
        if let Err(error) = result {
            if endpoint_failure {
                return Ok(());
            }
            return Err(error.into());
        }
        self.state = State::Running(running);
        self.pump_http_requests()
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
        budget: u32,
    ) -> Result<Option<BrowserRunResult>, RuntimeError> {
        let State::Running(running) = &mut self.state else {
            return Ok(None);
        };
        if let Some(size) = controller.take_resize()
            && let Some(slot) = running.console_slot
        {
            running
                .machine
                .resize_virtio_console(slot, size.columns, size.rows)?;
        }
        let mut input = [0; 128];
        let input_limit = if running.console_slot.is_some() {
            input.len()
        } else {
            input.len().min(running.machine.receive_space())
        };
        let count = controller.read_console(&mut input[..input_limit]);
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
            running.deliver_event(event)?;
        }
        // Every entry samples host time before the C core resumes guest execution.
        running.machine.update_time(timer_ticks, host_nanoseconds);
        let outcome = running.machine.run(budget.min(self.policy.yield_cycles));
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
        let delay = if outcome.state == RunState::Waiting {
            running
                .machine
                .sleep_duration_ms(self.policy.maximum_delay_ms)
        } else {
            0
        };
        self.pump_http_requests()?;
        Ok(Some(BrowserRunResult {
            cycles: outcome.cycles,
            state: outcome.state,
            delay_ms: delay,
        }))
    }

    #[must_use]
    pub fn next_timer_delay_ticks(&mut self, ticks: u64) -> Option<u64> {
        // Querying at the driver's current tick also updates pending interrupts.
        let State::Running(running) = &mut self.state else {
            return None;
        };
        running
            .machine
            .update_time(ticks, ticks.saturating_mul(100));
        running.machine.next_timer_delay_ticks()
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
        let (ninep_slots, ninep_endpoints) = Self::add_filesystems(&mut machine, &config)?;
        let console_slot = if config.console == Console::Virtio {
            Some(machine.add_console_device(80, 25)?)
        } else {
            None
        };
        let network_output = Rc::new(RefCell::new(VecDeque::new()));
        let network_slot = if loading.start.has_network && !config.networks.is_empty() {
            let mac = machine.generate_network_mac()?;
            let slot =
                machine.add_network_device(Box::new(OutputNetwork(network_output.clone())), mac)?;
            Some(slot)
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
            pointer_dimensions: dimensions,
            network_output,
            block_slots,
            ninep_slots,
            ninep_endpoints,
            pending_http: BTreeMap::new(),
        }));
        self.pump_ninep_transport_actions()?;
        self.actions.push_back(HostAction::Started);
        self.pump_http_requests()
    }

    fn add_filesystems(
        machine: &mut Machine,
        config: &VmConfig,
    ) -> Result<(Vec<usize>, BTreeMap<NinePEndpointId, usize>), RuntimeError> {
        let mut slots = Vec::new();
        let mut endpoints = BTreeMap::new();
        for (index, filesystem) in config.filesystems.iter().enumerate() {
            let endpoint = NinePEndpointId(
                u32::try_from(index + 1)
                    .map_err(|_| RuntimeError::InvalidConfig("too many 9p endpoints".into()))?,
            );
            let backend: Box<dyn NinePBackend> =
                Box::new(BrowserNineP::new(endpoint, filesystem.server.clone()));
            let slot = machine.add_ninep_device(backend, filesystem.tag.as_bytes())?;
            slots.push(slot);
            endpoints.insert(endpoint, slot);
        }
        Ok((slots, endpoints))
    }

    fn pump_http_requests(&mut self) -> Result<(), RuntimeError> {
        let State::Running(mut running) = core::mem::replace(&mut self.state, State::Idle) else {
            return Ok(());
        };
        for slot in running.block_slots.clone() {
            while let Some(request) = running.machine.next_http_block_request(slot)? {
                let id = self.allocate_request(&request.url);
                running.pending_http.insert(
                    id,
                    PendingHttp::Block {
                        slot,
                        request: request.id,
                    },
                );
            }
        }
        self.state = State::Running(running);
        self.pump_ninep_transport_actions()
    }

    fn pump_ninep_transport_actions(&mut self) -> Result<(), RuntimeError> {
        let State::Running(running) = &mut self.state else {
            return Ok(());
        };
        for slot in running.ninep_slots.clone() {
            while let Some(action) = running.machine.next_ninep_transport_action(slot)? {
                self.actions.push_back(HostAction::NineP(action));
            }
        }
        Ok(())
    }
}

fn scale_pointer_coordinate(value: u32, extent: u32) -> u32 {
    if extent == 0 {
        return 0;
    }
    let value = value.min(extent - 1);
    u32::try_from(u64::from(value) * 32_768 / u64::from(extent)).unwrap_or(32_767)
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

#[cfg(test)]
mod tests {
    use super::{ActiveTurn, RateEstimate, TICKS_PER_SECOND, scale_pointer_coordinate};

    #[test]
    fn pointer_coordinates_are_clamped_and_scaled_to_the_tablet_range() {
        assert_eq!(scale_pointer_coordinate(0, 1280), 0);
        assert_eq!(scale_pointer_coordinate(640, 1280), 16_384);
        assert_eq!(scale_pointer_coordinate(2000, 1280), 32_742);
        assert_eq!(scale_pointer_coordinate(1, 0), 0);
    }

    #[test]
    fn deadline_budget_reaches_the_first_matching_tick() {
        for rate in [1_000_000, 300_000_000, 1_123_456_789] {
            for deadline in [1, 2, 997, 10_001] {
                let turn = ActiveTurn {
                    start_ticks: 17_300_000_000_000_000,
                    rate,
                    budget: i32::MAX as u32,
                    used: 123,
                    stalled_calls: 0,
                    calls: 0,
                    timer_exits: 0,
                    timer_intervals: Vec::new(),
                    record_timer_interval: false,
                    idle_delay_ticks: 0,
                    terminal: None,
                };
                let target = turn.used + u64::from(turn.cycles_to_deadline(deadline));
                let offset = (u128::from(turn.used) * TICKS_PER_SECOND / u128::from(rate))
                    + u128::from(deadline);
                assert!(u128::from(target) * TICKS_PER_SECOND / u128::from(rate) >= offset);
                assert!(u128::from(target - 1) * TICKS_PER_SECOND / u128::from(rate) < offset);
            }
        }
    }

    #[test]
    fn whole_turn_samples_replace_guess_and_decay_previous_measurements() {
        let mut rate = RateEstimate::default();
        rate.observe(100_000, 10.0);
        assert!((rate.cycles_per_second - 10_000_000.0).abs() < 0.001);
        rate.observe(50_000, 10.0);
        assert!(rate.cycles_per_second < 10_000_000.0);
        assert!(rate.cycles_per_second > 5_000_000.0);
        let measured = rate.cycles_per_second;
        rate.observe(0, 100.0);
        assert!((rate.cycles_per_second - measured).abs() < 0.001);
    }
}
