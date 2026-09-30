//! Browser startup and execution state independent of the JavaScript adapter.

use core::fmt;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use crate::browser_input::{BrowserEvent, BrowserInputQueue};
use crate::browser_storage::HttpBlockStore;
use crate::config::{Console, DriveConfig, VmConfig, resolve_asset_path};
use crate::entropy::{EntropyError, EntropySource, SharedEntropy};
use crate::host_block::{
    HostBlockGeneration, HostBlockOutcome, HostBlockProviderId, HostBlockRequest,
    HostBlockRequestId, HostBlockStore,
};
use crate::machine::{
    BootAddresses, BootImages, FramebufferConfig, FramebufferUpdate, Machine, MachineConfig,
    MachineError,
};
use crate::ninep::{Filesystem, FilesystemError, LoadTicket};
use crate::ninep_backend::{FilesystemAttachment, RustFilesystem, RustNineP};
use crate::platform::FinishStatus;
use crate::tinyemu_core::CpuRunExitReason;
use crate::virtio_devices::{DeviceError, InputKind, NetworkBackend};

pub type EntropyCallback = Rc<RefCell<dyn FnMut(&mut [u8]) -> Result<(), EntropyError>>>;

struct CallbackEntropy {
    callback: EntropyCallback,
}

impl EntropySource for CallbackEntropy {
    fn fill(&mut self, destination: &mut [u8]) -> Result<(), EntropyError> {
        (self.callback.borrow_mut())(destination)
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
    HostBlock(HostBlockAction),
    Halted(LifecycleCause),
    Reset(LifecycleCause),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostBlockAction {
    Request(HostBlockRequest),
    Reset {
        provider: HostBlockProviderId,
        generation: HostBlockGeneration,
    },
    Close {
        provider: HostBlockProviderId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LifecycleCause {
    GuestPoweroff,
    GuestReboot,
    HostHalt,
    HostReset,
    HostBoot,
    GuestFailure,
}

const KEY_POWER: u16 = 116;
const KEY_RESTART: u16 = 408;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformRunResult {
    pub consumed_cycles: u32,
    pub state: CpuRunExitReason,
}

const GUEST_TICKS_PER_SECOND: u128 = 10_000_000;
const GUEST_TICKS_PER_MILLISECOND: u64 = 10_000;
const NANOSECONDS_PER_GUEST_TICK: u64 = 100;
const MAX_WFI_WAKE_DELAY_GUEST_TICKS: u64 = 1_000_000;
const CALIBRATION_HALFLIFE_HOST_MS: f64 = 10_000.0;
const INITIAL_EMULATED_CYCLES_PER_HOST_SECOND: f64 = 300_000_000.0;
const SKEW_BUCKETS: usize = 101;

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
        mantissa
            .checked_shl(u32::try_from(exponent - 52).expect("shift fits"))
            .unwrap_or(u64::MAX)
    } else {
        mantissa >> u32::try_from(52 - exponent).expect("shift fits")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuantumStart {
    Ready,
    VmInactive,
    AlreadyActive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuantumOutcome {
    HostServiceRequired,
    BudgetReached,
    WfiSleep,
    VmInactive,
}

#[derive(Clone, Copy, Debug)]
struct RateEstimate {
    consumed_cycles: f64,
    host_elapsed_ms: f64,
    emulated_cycles_per_host_second: f64,
}

impl Default for RateEstimate {
    fn default() -> Self {
        Self {
            consumed_cycles: 0.0,
            host_elapsed_ms: 0.0,
            emulated_cycles_per_host_second: INITIAL_EMULATED_CYCLES_PER_HOST_SECOND,
        }
    }
}

impl RateEstimate {
    fn observe(&mut self, consumed_cycles: u64, host_elapsed_ms: f64) {
        if consumed_cycles == 0 || !host_elapsed_ms.is_finite() || host_elapsed_ms <= 0.0 {
            return;
        }
        let decay = 2.0_f64.powf(-host_elapsed_ms / CALIBRATION_HALFLIFE_HOST_MS);
        // Decaying both totals weights each quantum by its whole cycle count.
        self.consumed_cycles = self.consumed_cycles * decay + integer_as_f64(consumed_cycles);
        self.host_elapsed_ms = self.host_elapsed_ms * decay + host_elapsed_ms;
        self.emulated_cycles_per_host_second =
            self.consumed_cycles * 1_000.0 / self.host_elapsed_ms;
    }
}

#[derive(Debug)]
struct AdaptiveSkew {
    weights: [f64; SKEW_BUCKETS],
    last_host_epoch_ms: Option<u64>,
    fraction: f64,
}

impl Default for AdaptiveSkew {
    fn default() -> Self {
        Self {
            weights: [0.0; SKEW_BUCKETS],
            last_host_epoch_ms: None,
            fraction: 0.20,
        }
    }
}

impl AdaptiveSkew {
    fn observe(&mut self, required: f64, host_epoch_ms: u64) {
        // Recent runnable quanta have more weight in the bounded histogram.
        if let Some(previous) = self.last_host_epoch_ms {
            let elapsed = host_epoch_ms.saturating_sub(previous);
            let decay = 2.0_f64.powf(-integer_as_f64(elapsed) / CALIBRATION_HALFLIFE_HOST_MS);
            for weight in &mut self.weights {
                *weight *= decay;
            }
        }
        self.last_host_epoch_ms = Some(host_epoch_ms);
        let bucket = rounded_positive_integer((required * 100.0).ceil()).min(100) as usize;
        self.weights[bucket] += 1.0;

        // The upper 99th percentile limits clock lead after fast quanta.
        let target = self.weights.iter().sum::<f64>() * 0.99;
        let mut accumulated = 0.0;
        for (index, weight) in self.weights.iter().enumerate() {
            accumulated += weight;
            if accumulated >= target {
                let percent = u32::try_from(index.min(99)).unwrap_or(99);
                self.fraction = f64::from(percent) / 100.0;
                break;
            }
        }
    }
}

fn guest_tick_rate(skew: f64, estimated_rate: f64, previous_rate: f64, carried: bool) -> u64 {
    // The previous measured cycle rate can only slow guest time in a quantum
    // that starts with clock lead. Otherwise the P99 skew sets the rate.
    let nominal = integer_as_f64(GUEST_TICKS_PER_MILLISECOND) * 1_000.0;
    let skewed = nominal * (1.0 - skew);
    let selected = if carried {
        skewed.min(nominal * estimated_rate / previous_rate)
    } else {
        skewed
    };
    rounded_positive_integer(selected.round().max(1.0))
}

#[derive(Debug)]
struct ActiveQuantum {
    start_guest_ticks: u64,
    cycles_per_host_second: u64,
    guest_ticks_per_host_second: u64,
    applied_guest_clock_skew: f64,
    carried_guest_ticks: u64,
    budget_cycles: u32,
    consumed_cycles: u64,
    stalled_cpu_runs: u32,
    cpu_runs: u32,
    timer_reprogramming_exits: u32,
    timer_intervals_guest_ticks: Vec<u64>,
    record_timer_interval: bool,
    wfi_wake_delay_guest_ticks: u64,
    terminal: Option<QuantumOutcome>,
}

impl ActiveQuantum {
    fn guest_ticks(&self) -> u64 {
        // One integer mapping owns guest time for the complete quantum.
        let offset = u128::from(self.consumed_cycles)
            * u128::from(self.guest_ticks_per_host_second)
            / u128::from(self.cycles_per_host_second);
        self.start_guest_ticks
            .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX))
    }

    fn cycles_until_deadline(&self, remaining_guest_ticks: u64) -> u32 {
        // Ceiling inversion reaches the first cycle whose guest tick is due.
        let offset = u128::from(self.guest_ticks() - self.start_guest_ticks)
            + u128::from(remaining_guest_ticks);
        let guest_tick_rate = u128::from(self.guest_ticks_per_host_second);
        let numerator = offset
            .saturating_mul(u128::from(self.cycles_per_host_second))
            .saturating_add(guest_tick_rate - 1);
        let target = numerator / guest_tick_rate;
        let additional = target
            .saturating_sub(u128::from(self.consumed_cycles))
            .max(1);
        u32::try_from(additional).unwrap_or(u32::MAX)
    }

    fn required_guest_clock_skew(&self, host_epoch_ms: u64) -> f64 {
        if self.terminal != Some(QuantumOutcome::BudgetReached) {
            return 0.0;
        }
        // Compare this runnable quantum with a zero-skew clock. The reported
        // fraction is the smallest slowdown that keeps its end at host time.
        let unskewed_ticks = u128::from(self.consumed_cycles)
            .saturating_mul(GUEST_TICKS_PER_SECOND)
            / u128::from(self.cycles_per_host_second);
        let unskewed_ticks = u64::try_from(unskewed_ticks).unwrap_or(u64::MAX);
        let available_ticks = host_epoch_ms
            .saturating_mul(GUEST_TICKS_PER_MILLISECOND)
            .saturating_sub(self.start_guest_ticks);
        if unskewed_ticks > available_ticks {
            integer_as_f64(unskewed_ticks - available_ticks) / integer_as_f64(unskewed_ticks)
        } else {
            0.0
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct QuantumStatistics {
    cpu_runs: u32,
    timer_reprogramming_exits: u32,
    median_timer_interval_guest_ticks: u64,
    consumed_cycles: u64,
    required_guest_clock_skew: f64,
    applied_guest_clock_skew: f64,
    carried_guest_ticks: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeError {
    AlreadyStarted,
    UnexpectedResponse(u32),
    HttpStatus(u16),
    InvalidConfig(String),
    Machine(String),
    Filesystem(FilesystemError),
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

struct BootState {
    firmware: Option<Vec<u8>>,
    kernel: Option<Vec<u8>>,
    initrd: Option<Vec<u8>>,
    command_line: String,
    addresses: BootAddresses,
}

impl BootState {
    fn images(&self) -> BootImages<'_> {
        BootImages {
            firmware: self.firmware.as_deref(),
            kernel: self.kernel.as_deref(),
            initrd: self.initrd.as_deref(),
            command_line: &self.command_line,
        }
    }
}

struct Running {
    machine: Machine,
    boot: BootState,
    console_slot: Option<usize>,
    uart_output: bool,
    network_slot: Option<usize>,
    keyboard_slot: Option<usize>,
    power_slot: usize,
    pointer_slot: Option<usize>,
    pointer_dimensions: Option<(u32, u32)>,
    network_output: Rc<RefCell<VecDeque<Vec<u8>>>>,
    block_slots: Vec<usize>,
    host_blocks: BTreeMap<HostBlockProviderId, (usize, HostBlockGeneration)>,
    _filesystem_attachments: Vec<FilesystemAttachment>,
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
    VmInactive,
    Config {
        start: RuntimeStart,
        request_id: u32,
    },
    Loading(Box<Loading>),
    Running(Box<Running>),
    Halted(Box<Running>),
}

pub struct BrowserRuntime {
    state: State,
    filesystems: BTreeMap<String, RustFilesystem>,
    next_request_id: u32,
    actions: VecDeque<HostAction>,
    retired_http: BTreeSet<u32>,
    entropy: Option<EntropyCallback>,
    target_quantum_ms: f64,
    adaptive_skew: AdaptiveSkew,
    diagnostics: bool,
    cycle_rate_estimate: RateEstimate,
    previous_fast_quantum_rate: Option<f64>,
    guest_clock_floor_ticks: u64,
    active_quantum: Option<ActiveQuantum>,
    last_quantum: QuantumStatistics,
}

impl Default for BrowserRuntime {
    fn default() -> Self {
        Self {
            state: State::VmInactive,
            filesystems: BTreeMap::new(),
            next_request_id: 1,
            actions: VecDeque::new(),
            retired_http: BTreeSet::new(),
            entropy: None,
            target_quantum_ms: 20.0,
            adaptive_skew: AdaptiveSkew::default(),
            diagnostics: false,
            cycle_rate_estimate: RateEstimate::default(),
            previous_fast_quantum_rate: None,
            guest_clock_floor_ticks: 0,
            active_quantum: None,
            last_quantum: QuantumStatistics::default(),
        }
    }
}

impl BrowserRuntime {
    /// Registers a namespace before startup; configured endpoints use its key.
    ///
    /// # Errors
    /// Returns an error for duplicate keys or a VM that has begun startup.
    pub fn register_filesystem(
        &mut self,
        key: String,
        filesystem: Filesystem,
    ) -> Result<(), RuntimeError> {
        self.register_filesystem_handle(key, RustFilesystem::new(filesystem))
    }

    /// Registers an independently owned filesystem handle before startup.
    ///
    /// # Errors
    /// Returns an error for duplicate keys or a VM that has begun startup.
    pub fn register_filesystem_handle(
        &mut self,
        key: String,
        filesystem: RustFilesystem,
    ) -> Result<(), RuntimeError> {
        if !matches!(self.state, State::VmInactive) || self.filesystems.contains_key(&key) {
            return Err(RuntimeError::InvalidConfig(
                "filesystem registration requires an unused key before startup".into(),
            ));
        }
        self.filesystems.insert(key, filesystem);
        Ok(())
    }

    /// Removes a binding after VM teardown without destroying its namespace.
    ///
    /// # Errors
    /// Returns an error while VM resources or startup are active.
    pub fn unregister_filesystem(&mut self, key: &str) -> Result<(), RuntimeError> {
        if !matches!(self.state, State::VmInactive) {
            return Err(RuntimeError::AlreadyStarted);
        }
        self.filesystems.remove(key);
        Ok(())
    }

    /// Publishes host namespace changes after its borrow has ended.
    ///
    /// # Errors
    /// Returns an error for invalid backend results or guest descriptors.
    pub fn poll_filesystems(&mut self) -> Result<(), RuntimeError> {
        if let State::Running(running) | State::Halted(running) = &mut self.state {
            running.machine.poll_ninep()?;
        }
        Ok(())
    }

    /// Mutates a registered tree, then publishes any newly satisfied guest I/O.
    ///
    /// # Errors
    /// Returns an error for unknown keys, failed host operations, or guest I/O.
    pub fn with_filesystem<T>(
        &mut self,
        key: &str,
        operation: impl FnOnce(&mut Filesystem) -> Result<T, FilesystemError>,
    ) -> Result<T, RuntimeError> {
        let tree = self
            .filesystems
            .get(key)
            .ok_or_else(|| RuntimeError::InvalidConfig("unknown filesystem".into()))?;
        let result = tree.with_filesystem(operation);
        // Even a failed operation can settle a load as failed. Poll after the
        // namespace borrow ends, and while the machine's CPU is not borrowed.
        if let State::Running(running) | State::Halted(running) = &mut self.state {
            running.machine.poll_ninep()?;
        }
        result.map_err(RuntimeError::Filesystem)
    }

    /// Source work is dispatched by the host only after execution returns.
    #[must_use]
    pub fn next_ninep_load(&self) -> Option<(String, LoadTicket)> {
        self.filesystems
            .iter()
            .find_map(|(key, tree)| tree.next_load().map(|ticket| (key.clone(), ticket)))
    }

    /// Configures the duration and optional counters before a quantum starts.
    ///
    /// # Errors
    /// Returns an error for an invalid duration or an active quantum.
    pub fn configure_quantum(
        &mut self,
        target_quantum_ms: f64,
        diagnostics: bool,
    ) -> Result<(), RuntimeError> {
        if !target_quantum_ms.is_finite()
            || target_quantum_ms <= 0.0
            || target_quantum_ms > 100.0
            || self.active_quantum.is_some()
        {
            return Err(RuntimeError::InvalidConfig(
                "invalid quantum duration".into(),
            ));
        }
        self.target_quantum_ms = target_quantum_ms;
        self.diagnostics = diagnostics;
        Ok(())
    }

    #[must_use]
    pub fn wake_delay_ms(&self, _host_epoch_ms: u64, requested_wakeup_delay_ms: u32) -> u32 {
        requested_wakeup_delay_ms
    }

    #[must_use]
    pub fn begin_quantum(&mut self, host_epoch_ms: u64) -> QuantumStart {
        if self.active_quantum.is_some() {
            return QuantumStart::AlreadyActive;
        }
        if !matches!(self.state, State::Running(_)) {
            return QuantumStart::VmInactive;
        }
        let locked_rate_cycles_per_host_second = rounded_positive_integer(
            self.cycle_rate_estimate
                .emulated_cycles_per_host_second
                .max(1.0),
        );
        // Carry clock lead into the duration and budget of this quantum.
        let host_ticks = host_epoch_ms.saturating_mul(GUEST_TICKS_PER_MILLISECOND);
        for tree in self.filesystems.values() {
            tree.with_filesystem(|filesystem| filesystem.set_time(host_epoch_ms / 1_000));
        }
        let carried_ticks = self.guest_clock_floor_ticks.saturating_sub(host_ticks);
        let target_ms = self.target_quantum_ms
            + integer_as_f64(carried_ticks) / integer_as_f64(GUEST_TICKS_PER_MILLISECOND);
        let estimated_rate = self.cycle_rate_estimate.emulated_cycles_per_host_second;
        let previous_rate = self.previous_fast_quantum_rate.unwrap_or(estimated_rate);
        let budget_rate = if carried_ticks == 0 {
            estimated_rate
        } else {
            estimated_rate.max(previous_rate)
        };
        let quantum_budget_cycles = rounded_positive_integer(
            (budget_rate * target_ms / 1_000.0)
                .round()
                .clamp(1.0, f64::from(i32::MAX)),
        );
        let quantum_budget_cycles = u32::try_from(quantum_budget_cycles).unwrap_or(i32::MAX as u32);
        // Use the slower clock mapping when the prior runnable quantum was
        // faster than the long-term rate and left guest time ahead.
        let guest_ticks_per_host_second = guest_tick_rate(
            self.adaptive_skew.fraction,
            estimated_rate,
            previous_rate,
            carried_ticks != 0,
        );
        let applied_guest_clock_skew = 1.0
            - integer_as_f64(guest_ticks_per_host_second)
                / (integer_as_f64(GUEST_TICKS_PER_MILLISECOND) * 1_000.0);
        self.active_quantum = Some(ActiveQuantum {
            start_guest_ticks: host_ticks.max(self.guest_clock_floor_ticks),
            cycles_per_host_second: locked_rate_cycles_per_host_second,
            guest_ticks_per_host_second,
            applied_guest_clock_skew,
            carried_guest_ticks: carried_ticks,
            budget_cycles: quantum_budget_cycles,
            consumed_cycles: 0,
            stalled_cpu_runs: 0,
            cpu_runs: 0,
            timer_reprogramming_exits: 0,
            timer_intervals_guest_ticks: Vec::new(),
            record_timer_interval: false,
            wfi_wake_delay_guest_ticks: 0,
            terminal: None,
        });
        QuantumStart::Ready
    }

    /// Runs until browser work or the quantum boundary needs JavaScript.
    ///
    /// # Errors
    /// Returns an error for invalid guest I/O or an execution loop without progress.
    pub fn run_quantum(
        &mut self,
        input_queue: &mut BrowserInputQueue,
    ) -> Result<QuantumOutcome, RuntimeError> {
        let Some(mut quantum) = self.active_quantum.take() else {
            return Err(RuntimeError::Machine(
                "advance without an active quantum".into(),
            ));
        };
        let result = self.run_active_quantum(input_queue, &mut quantum);
        self.active_quantum = Some(quantum);
        result
    }

    fn run_active_quantum(
        &mut self,
        input_queue: &mut BrowserInputQueue,
        quantum: &mut ActiveQuantum,
    ) -> Result<QuantumOutcome, RuntimeError> {
        if let Some(terminal) = quantum.terminal {
            return Ok(terminal);
        }
        while quantum.consumed_cycles < u64::from(quantum.budget_cycles) {
            // Timer deadlines stay in guest ticks until the CPU-run limit is known.
            let guest_ticks = quantum.guest_ticks();
            let deadline_remaining_guest_ticks = self.next_timer_remaining_guest_ticks(guest_ticks);
            let quantum_remaining_cycles =
                u64::from(quantum.budget_cycles) - quantum.consumed_cycles;
            let mut cpu_run_cycle_limit =
                u32::try_from(quantum_remaining_cycles).unwrap_or(i32::MAX as u32);
            if let Some(remaining_guest_ticks) = deadline_remaining_guest_ticks {
                cpu_run_cycle_limit =
                    cpu_run_cycle_limit.min(quantum.cycles_until_deadline(remaining_guest_ticks));
                if quantum.record_timer_interval
                    && self.diagnostics
                    && quantum.timer_intervals_guest_ticks.len() < 10_000
                {
                    quantum
                        .timer_intervals_guest_ticks
                        .push(remaining_guest_ticks);
                }
            }
            quantum.record_timer_interval = false;
            let Some(outcome) = self.run_cpu_once(
                input_queue,
                guest_ticks,
                guest_ticks.saturating_mul(NANOSECONDS_PER_GUEST_TICK),
                cpu_run_cycle_limit,
            )?
            else {
                quantum.terminal = Some(QuantumOutcome::VmInactive);
                return Ok(QuantumOutcome::VmInactive);
            };
            quantum.consumed_cycles += u64::from(outcome.consumed_cycles);
            // The interpreter can overshoot a block boundary; actual cycles
            // determine both later guest time and the next calibration sample.
            if self.diagnostics {
                quantum.cpu_runs += 1;
            }
            quantum.stalled_cpu_runs = if outcome.consumed_cycles == 0 {
                quantum.stalled_cpu_runs + 1
            } else {
                0
            };
            if quantum.stalled_cpu_runs > 1
                || outcome.state == CpuRunExitReason::CycleLimitReached
                    && outcome.consumed_cycles == 0
            {
                return Err(RuntimeError::Machine("CPU made no progress".into()));
            }
            if outcome.state == CpuRunExitReason::TimerReprogrammed {
                quantum.record_timer_interval = true;
                if self.diagnostics {
                    quantum.timer_reprogramming_exits += 1;
                }
            }
            if outcome.state == CpuRunExitReason::WfiSleep {
                // Pending timers are already reflected in the machine state.
                // Only an actual WFI sleep leaves this quantum waiting on the host.
                let next_timer_remaining_guest_ticks =
                    self.next_timer_remaining_guest_ticks(quantum.guest_ticks());
                if let State::Running(running) = &self.state
                    && running.machine.is_wfi_sleeping()
                {
                    quantum.wfi_wake_delay_guest_ticks = next_timer_remaining_guest_ticks
                        .unwrap_or(u64::MAX)
                        .min(MAX_WFI_WAKE_DELAY_GUEST_TICKS);
                    quantum.terminal = Some(QuantumOutcome::WfiSleep);
                }
            }
            if quantum.consumed_cycles >= u64::from(quantum.budget_cycles)
                && quantum.terminal.is_none()
            {
                quantum.terminal = Some(QuantumOutcome::BudgetReached);
            }
            if !self.actions.is_empty() || outcome.state == CpuRunExitReason::HostServiceRequested {
                return Ok(QuantumOutcome::HostServiceRequired);
            }
            if let Some(terminal) = quantum.terminal {
                return Ok(terminal);
            }
        }
        quantum.terminal = Some(QuantumOutcome::BudgetReached);
        Ok(QuantumOutcome::BudgetReached)
    }

    /// Ends a completed quantum and incorporates its elapsed host time.
    ///
    /// # Errors
    /// Returns an error when a quantum is unfinished or elapsed time is invalid.
    pub fn finish_quantum(
        &mut self,
        host_elapsed_ms: f64,
        host_epoch_ms: u64,
    ) -> Result<u32, RuntimeError> {
        if !host_elapsed_ms.is_finite() || host_elapsed_ms < 0.0 {
            return Err(RuntimeError::InvalidConfig(
                "invalid quantum elapsed time".into(),
            ));
        }
        let Some(quantum) = self.active_quantum.take() else {
            return Err(RuntimeError::Machine(
                "finish without an active quantum".into(),
            ));
        };
        if quantum.terminal.is_none() {
            self.active_quantum = Some(quantum);
            return Err(RuntimeError::Machine("unfinished browser quantum".into()));
        }
        self.guest_clock_floor_ticks = quantum.guest_ticks();
        let host_ticks = host_epoch_ms.saturating_mul(GUEST_TICKS_PER_MILLISECOND);
        self.previous_fast_quantum_rate = if quantum.terminal == Some(QuantumOutcome::BudgetReached)
            && self.guest_clock_floor_ticks > host_ticks
            && host_elapsed_ms > 0.0
            && quantum.consumed_cycles > 0
        {
            Some(integer_as_f64(quantum.consumed_cycles) * 1_000.0 / host_elapsed_ms)
        } else {
            None
        };
        self.cycle_rate_estimate
            .observe(quantum.consumed_cycles, host_elapsed_ms);
        let required_guest_clock_skew = quantum.required_guest_clock_skew(host_epoch_ms);
        if quantum.terminal == Some(QuantumOutcome::BudgetReached) {
            self.adaptive_skew
                .observe(required_guest_clock_skew, host_epoch_ms);
        }
        if self.diagnostics {
            let mut intervals = quantum.timer_intervals_guest_ticks;
            intervals.sort_unstable();
            self.last_quantum = QuantumStatistics {
                cpu_runs: quantum.cpu_runs,
                timer_reprogramming_exits: quantum.timer_reprogramming_exits,
                median_timer_interval_guest_ticks: intervals
                    .get(intervals.len() / 2)
                    .copied()
                    .unwrap_or(0),
                consumed_cycles: quantum.consumed_cycles,
                required_guest_clock_skew,
                applied_guest_clock_skew: quantum.applied_guest_clock_skew,
                carried_guest_ticks: quantum.carried_guest_ticks,
            };
        }
        let requested = if quantum.terminal == Some(QuantumOutcome::WfiSleep) {
            let due = self
                .guest_clock_floor_ticks
                .saturating_add(quantum.wfi_wake_delay_guest_ticks);
            let wall = host_epoch_ms.saturating_mul(GUEST_TICKS_PER_MILLISECOND);
            u32::try_from(
                due.saturating_sub(wall)
                    .div_ceil(GUEST_TICKS_PER_MILLISECOND),
            )
            .unwrap_or(u32::MAX)
        } else {
            0
        };
        Ok(requested)
    }

    pub fn abort_quantum(&mut self) {
        if let Some(quantum) = self.active_quantum.take() {
            self.guest_clock_floor_ticks = self.guest_clock_floor_ticks.max(quantum.guest_ticks());
        }
        self.previous_fast_quantum_rate = None;
    }

    #[must_use]
    pub fn timing_stat(&self, kind: u32) -> f64 {
        match kind {
            0 => self.cycle_rate_estimate.emulated_cycles_per_host_second,
            1 => f64::from(self.last_quantum.cpu_runs),
            2 => f64::from(self.last_quantum.timer_reprogramming_exits),
            3 => integer_as_f64(self.last_quantum.median_timer_interval_guest_ticks),
            4 => integer_as_f64(self.last_quantum.consumed_cycles),
            5 => self.last_quantum.required_guest_clock_skew,
            6 => self.last_quantum.applied_guest_clock_skew,
            7 => integer_as_f64(self.last_quantum.carried_guest_ticks),
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
        if !matches!(self.state, State::VmInactive) {
            return Err(RuntimeError::AlreadyStarted);
        }
        self.guest_clock_floor_ticks = 0;
        self.cycle_rate_estimate = RateEstimate::default();
        self.previous_fast_quantum_rate = None;
        self.adaptive_skew = AdaptiveSkew::default();
        self.last_quantum = QuantumStatistics::default();
        let request_id = self.allocate_request(&start.config_url);
        self.state = State::Config { start, request_id };
        Ok(())
    }

    /// Starts asset loading from a host-resolved configuration.
    ///
    /// # Errors
    /// Returns an error for an invalid configuration or an active VM.
    pub fn start_resolved(
        &mut self,
        start: RuntimeStart,
        config: VmConfig,
    ) -> Result<(), RuntimeError> {
        if !matches!(self.state, State::VmInactive) {
            return Err(RuntimeError::AlreadyStarted);
        }
        self.guest_clock_floor_ticks = 0;
        self.cycle_rate_estimate = RateEstimate::default();
        self.previous_fast_quantum_rate = None;
        self.adaptive_skew = AdaptiveSkew::default();
        self.last_quantum = QuantumStatistics::default();
        self.begin_loading(start, config)
    }

    fn begin_loading(&mut self, start: RuntimeStart, config: VmConfig) -> Result<(), RuntimeError> {
        let mut assets = VecDeque::new();
        for (kind, path) in [
            (AssetKind::Firmware, config.bios.as_deref()),
            (AssetKind::Kernel, config.kernel.as_deref()),
            (AssetKind::Initrd, config.initrd.as_deref()),
        ] {
            if let Some(path) = path {
                assets.push_back((kind, path.to_owned()));
            }
        }
        for (index, drive) in config.drives.iter().enumerate() {
            if let DriveConfig::Http { file, .. } = drive {
                assets.push_back((AssetKind::Drive(index), file.clone()));
            }
        }
        let drive_count = config.drives.len();
        self.state = State::Loading(Box::new(Loading {
            start,
            config: Some(config),
            assets,
            waiting: None,
            firmware: None,
            kernel: None,
            initrd: None,
            drive_manifests: (0..drive_count).map(|_| None).collect(),
        }));
        self.request_next_asset()
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
        if self.retired_http.remove(&id) {
            return Ok(());
        }
        if !(200..300).contains(&status) {
            return Err(RuntimeError::HttpStatus(status));
        }
        let halted = matches!(self.state, State::Halted(_));
        let state = core::mem::replace(&mut self.state, State::VmInactive);
        match state {
            State::Config { start, request_id } if request_id == id => {
                let source = String::from_utf8(bytes).map_err(|_| {
                    RuntimeError::InvalidConfig("configuration is not UTF-8".into())
                })?;
                let mut config = VmConfig::parse(&source)
                    .map_err(|error| RuntimeError::InvalidConfig(error.to_string()))?;
                if config
                    .drives
                    .iter()
                    .any(|drive| matches!(drive, DriveConfig::Host { .. }))
                {
                    return Err(RuntimeError::InvalidConfig(
                        "host block drives require resolved startup".into(),
                    ));
                }
                if !start.command_line.is_empty() {
                    config.apply_command_line(&start.command_line);
                }
                for value in [&mut config.bios, &mut config.kernel, &mut config.initrd]
                    .into_iter()
                    .flatten()
                {
                    *value = resolve_asset_path(Some(&start.config_url), value);
                }
                for drive in &mut config.drives {
                    if let DriveConfig::Http { file, .. } = drive {
                        *file = resolve_asset_path(Some(&start.config_url), file);
                    }
                }
                self.begin_loading(start, config)
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
                        let url = loading
                            .config
                            .as_ref()
                            .ok_or_else(|| {
                                RuntimeError::InvalidConfig("missing configuration".into())
                            })?
                            .drives
                            .get(index)
                            .ok_or_else(|| {
                                RuntimeError::InvalidConfig("invalid drive index".into())
                            })?
                            .clone();
                        let DriveConfig::Http { file: url, .. } = url else {
                            return Err(RuntimeError::InvalidConfig("invalid drive source".into()));
                        };
                        loading.drive_manifests[index] = Some((url, bytes));
                    }
                }
                self.state = State::Loading(loading);
                self.request_next_asset()
            }
            State::Running(mut running) | State::Halted(mut running) => {
                let Some(pending) = running.pending_http.remove(&id) else {
                    self.state = if halted {
                        State::Halted(running)
                    } else {
                        State::Running(running)
                    };
                    return Err(RuntimeError::UnexpectedResponse(id));
                };
                match pending {
                    PendingHttp::Block { slot, request } => running
                        .machine
                        .complete_http_block_request(slot, request, bytes)?,
                }
                self.state = if halted {
                    State::Halted(running)
                } else {
                    State::Running(running)
                };
                if halted {
                    Ok(())
                } else {
                    self.pump_http_requests()
                }
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
    pub fn run_cpu_once(
        &mut self,
        input_queue: &mut BrowserInputQueue,
        guest_timer_ticks: u64,
        guest_rtc_ns: u64,
        cycle_limit_cycles: u32,
    ) -> Result<Option<PlatformRunResult>, RuntimeError> {
        let State::Running(running) = &mut self.state else {
            return Ok(None);
        };
        if let Some(size) = input_queue.take_resize()
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
        let count = input_queue.read_console(&mut input[..input_limit]);
        if count != 0 {
            if let Some(slot) = running.console_slot {
                running
                    .machine
                    .virtio_console_receive(slot, &input[..count])?;
            } else {
                running.machine.receive_console(&input[..count]);
            }
        }
        while let Some(event) = input_queue.next_event() {
            running.deliver_event(event)?;
        }
        // Every entry samples host time before the C core resumes guest execution.
        running
            .machine
            .present_guest_clocks(guest_timer_ticks, guest_rtc_ns);
        let outcome = running.machine.run_cpu(cycle_limit_cycles);
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
        self.pump_http_requests()?;
        let finish = match &self.state {
            State::Running(running) => running.machine.finish_status(),
            _ => FinishStatus::Running,
        };
        if finish == FinishStatus::Reset {
            self.reset_machine(LifecycleCause::GuestReboot)?;
            *input_queue = BrowserInputQueue::default();
        } else if finish != FinishStatus::Running {
            let State::Running(running) = core::mem::replace(&mut self.state, State::VmInactive)
            else {
                unreachable!();
            };
            self.state = State::Halted(running);
            let cause = if matches!(finish, FinishStatus::Failed(_)) {
                LifecycleCause::GuestFailure
            } else {
                LifecycleCause::GuestPoweroff
            };
            self.actions.push_back(HostAction::Halted(cause));
            return Ok(None);
        }
        Ok(Some(PlatformRunResult {
            consumed_cycles: outcome.consumed_cycles,
            state: outcome.state,
        }))
    }

    #[must_use]
    pub fn next_timer_remaining_guest_ticks(&mut self, ticks: u64) -> Option<u64> {
        // Querying at the driver's current tick also updates pending interrupts.
        let State::Running(running) = &mut self.state else {
            return None;
        };
        running
            .machine
            .present_guest_clocks(ticks, ticks.saturating_mul(100));
        running.machine.next_timer_remaining_guest_ticks()
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

    #[must_use]
    pub fn is_halted(&self) -> bool {
        matches!(self.state, State::Halted(_))
    }

    /// Stops CPU execution without disposing of the machine or its backends.
    ///
    /// # Errors
    /// Returns an error unless a VM is running between CPU calls.
    pub fn halt(&mut self) -> Result<(), RuntimeError> {
        if self.active_quantum.is_some() {
            return Err(RuntimeError::Machine("quantum is active".into()));
        }
        let running = match core::mem::replace(&mut self.state, State::VmInactive) {
            State::Running(running) => running,
            other => {
                self.state = other;
                return Err(RuntimeError::Machine("VM is not running".into()));
            }
        };
        self.state = State::Halted(running);
        self.actions
            .push_back(HostAction::Halted(LifecycleCause::HostHalt));
        Ok(())
    }

    /// Boots a halted VM or forcibly resets a running VM in place.
    ///
    /// # Errors
    /// Returns an error for an invalid state or failed CPU or boot reset.
    pub fn reset(&mut self) -> Result<(), RuntimeError> {
        if self.active_quantum.is_some() {
            return Err(RuntimeError::Machine("quantum is active".into()));
        }
        let cause = if self.is_halted() {
            LifecycleCause::HostBoot
        } else {
            LifecycleCause::HostReset
        };
        self.reset_machine(cause)
    }

    /// Delivers a power key to the guest; shutdown depends on guest policy.
    ///
    /// # Errors
    /// Returns an error unless a VM is running or input delivery fails.
    pub fn request_shutdown(&mut self) -> Result<(), RuntimeError> {
        self.request_power_key(KEY_POWER)
    }

    /// Delivers a restart key to the guest; reboot depends on guest policy.
    ///
    /// # Errors
    /// Returns an error unless a VM is running or input delivery fails.
    pub fn request_reboot(&mut self) -> Result<(), RuntimeError> {
        self.request_power_key(KEY_RESTART)
    }

    fn request_power_key(&mut self, code: u16) -> Result<(), RuntimeError> {
        let State::Running(running) = &mut self.state else {
            return Err(RuntimeError::Machine("VM is not running".into()));
        };
        running
            .machine
            .virtio_key_event(running.power_slot, code, true)?;
        running
            .machine
            .virtio_key_event(running.power_slot, code, false)?;
        Ok(())
    }

    /// Disposes of a halted VM and releases its attached machine resources.
    ///
    /// # Errors
    /// Returns an error unless a VM is halted.
    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        let running = match core::mem::replace(&mut self.state, State::VmInactive) {
            State::Halted(running) => running,
            other => {
                self.state = other;
                return Err(RuntimeError::Machine("VM is not halted".into()));
            }
        };
        self.retired_http.extend(running.pending_http.keys());
        self.actions.clear();
        self.actions.extend(
            running
                .host_blocks
                .keys()
                .copied()
                .map(|provider| HostAction::HostBlock(HostBlockAction::Close { provider })),
        );
        Ok(())
    }

    fn reset_machine(&mut self, cause: LifecycleCause) -> Result<(), RuntimeError> {
        let state = core::mem::replace(&mut self.state, State::VmInactive);
        let mut running = match state {
            State::Running(running) | State::Halted(running) => running,
            other => {
                self.state = other;
                return Err(RuntimeError::Machine("VM is not initialized".into()));
            }
        };
        self.retired_http.extend(running.pending_http.keys());
        running.pending_http.clear();
        running.machine.reset()?;
        running
            .machine
            .load_boot_at(running.boot.images(), running.boot.addresses)?;
        running.network_output.borrow_mut().clear();
        self.state = State::Running(running);
        self.actions.push_back(HostAction::Reset(cause));
        self.pump_host_block_actions()?;
        self.pump_http_requests()
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
        let State::Loading(mut loading) = core::mem::replace(&mut self.state, State::VmInactive)
        else {
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
        let mut host_blocks = BTreeMap::new();
        for (index, manifest) in loading.drive_manifests.into_iter().enumerate() {
            if let DriveConfig::Host {
                provider,
                capacity_sectors,
            } = &config.drives[index]
            {
                let provider = HostBlockProviderId(*provider);
                if host_blocks.contains_key(&provider) {
                    return Err(RuntimeError::InvalidConfig(
                        "duplicate block provider".into(),
                    ));
                }
                let store = HostBlockStore::new(provider, *capacity_sectors);
                let mut id = [0; 20];
                let name = format!("riscbox-host-{index}");
                id[..name.len()].copy_from_slice(name.as_bytes());
                let slot = machine.add_host_block_device(store, id)?;
                host_blocks.insert(provider, (slot, HostBlockGeneration(1)));
            } else if let Some((url, bytes)) = manifest {
                let source = String::from_utf8(bytes).map_err(|_| {
                    RuntimeError::InvalidConfig("drive manifest is not UTF-8".into())
                })?;
                let store = HttpBlockStore::from_manifest(&url, &source, 16 << 20)
                    .map_err(|error| RuntimeError::Machine(error.to_string()))?;
                let mut id = [0; 20];
                let name = format!("riscbox-http-{index}");
                id[..name.len()].copy_from_slice(name.as_bytes());
                block_slots.push(machine.add_http_block_device(store, id)?);
            }
        }
        // Claims are held by the VM rather than individual endpoints. Failed
        // construction drops these claims and closes any sessions already added.
        let mut trees: Vec<RustFilesystem> = Vec::new();
        let mut filesystem_attachments = Vec::new();
        for entry in &config.filesystems {
            if let Some(tree) = self.filesystems.get(&entry.server)
                && !trees.iter().any(|other| tree.same_tree(other))
            {
                filesystem_attachments.push(tree.attach().map_err(RuntimeError::Filesystem)?);
                trees.push(tree.clone());
            }
        }
        self.add_filesystems(&mut machine, &config)?;
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
        let power_slot = match keyboard_slot {
            Some(slot) => slot,
            None => machine.add_input_device(InputKind::Keyboard)?,
        };
        let boot = BootState {
            firmware: loading.firmware,
            kernel: loading.kernel,
            initrd: loading.initrd,
            command_line: config.command_line.clone().unwrap_or_default(),
            addresses: BootAddresses {
                firmware: config.bios_address,
                kernel: config.kernel_address,
                initrd: config.initrd_address,
                fdt: config.fdt_address,
            },
        };
        machine.load_boot_at(boot.images(), boot.addresses)?;
        self.state = State::Running(Box::new(Running {
            machine,
            boot,
            console_slot,
            uart_output: config.console == Console::Uart || config.uart_output,
            network_slot,
            keyboard_slot,
            power_slot,
            pointer_slot,
            pointer_dimensions: dimensions,
            network_output,
            block_slots,
            host_blocks,
            _filesystem_attachments: filesystem_attachments,
            pending_http: BTreeMap::new(),
        }));
        self.actions.push_back(HostAction::Started);
        self.pump_http_requests()
    }

    fn add_filesystems(
        &self,
        machine: &mut Machine,
        config: &VmConfig,
    ) -> Result<(), RuntimeError> {
        for filesystem in &config.filesystems {
            let tree = self.filesystems.get(&filesystem.server).ok_or_else(|| {
                RuntimeError::InvalidConfig(format!(
                    "9p filesystem is not bound: {}",
                    filesystem.server
                ))
            })?;
            let backend = Box::new(RustNineP::new(tree.clone()).map_err(RuntimeError::Filesystem)?);
            machine.add_ninep_device(backend, filesystem.tag.as_bytes())?;
        }
        Ok(())
    }

    fn pump_http_requests(&mut self) -> Result<(), RuntimeError> {
        let State::Running(mut running) = core::mem::replace(&mut self.state, State::VmInactive)
        else {
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
        self.pump_host_block_actions()?;
        Ok(())
    }

    fn pump_host_block_actions(&mut self) -> Result<(), RuntimeError> {
        let State::Running(running) = &mut self.state else {
            return Ok(());
        };
        for (provider, (slot, observed)) in &mut running.host_blocks {
            let generation = running.machine.host_block_generation(*slot)?;
            if generation != *observed {
                *observed = generation;
                self.actions
                    .push_back(HostAction::HostBlock(HostBlockAction::Reset {
                        provider: *provider,
                        generation,
                    }));
            }
            if let Some(request) = running.machine.next_host_block_request(*slot)? {
                self.actions
                    .push_back(HostAction::HostBlock(HostBlockAction::Request(request)));
            }
        }
        Ok(())
    }

    /// Completes a host provider request, ignoring retired generations.
    ///
    /// # Errors
    /// Returns an error for a wrong provider or unknown current request.
    pub fn complete_host_block(
        &mut self,
        provider: HostBlockProviderId,
        generation: HostBlockGeneration,
        id: HostBlockRequestId,
        result: HostBlockOutcome,
    ) -> Result<(), RuntimeError> {
        let halted = self.is_halted();
        let mut running = match core::mem::replace(&mut self.state, State::VmInactive) {
            State::Running(running) | State::Halted(running) => running,
            other => {
                self.state = other;
                return Err(RuntimeError::Machine(
                    "block completion without a VM".into(),
                ));
            }
        };
        let outcome = match running.host_blocks.get(&provider) {
            Some((slot, _)) => running
                .machine
                .complete_host_block_request(*slot, generation, id, result)
                .map_err(Into::into),
            None => Err(RuntimeError::InvalidConfig("unknown block provider".into())),
        };
        self.state = if halted {
            State::Halted(running)
        } else {
            State::Running(running)
        };
        outcome?;
        if !halted {
            self.pump_http_requests()?;
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

struct OutputNetwork(Rc<RefCell<VecDeque<Vec<u8>>>>);

impl NetworkBackend for OutputNetwork {
    fn transmit(&mut self, packet: &[u8]) -> Result<(), DeviceError> {
        self.0.borrow_mut().push_back(packet.to_vec());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveQuantum, AdaptiveSkew, BrowserRuntime, CALIBRATION_HALFLIFE_HOST_MS, QuantumOutcome,
        RateEstimate, guest_tick_rate, scale_pointer_coordinate,
    };

    #[test]
    fn quantum_target_is_configurable() {
        let mut runtime = BrowserRuntime::default();
        assert!((runtime.target_quantum_ms - 20.0).abs() < f64::EPSILON);
        runtime
            .configure_quantum(25.0, false)
            .expect("custom duration");
        assert!((runtime.target_quantum_ms - 25.0).abs() < f64::EPSILON);

        assert!((CALIBRATION_HALFLIFE_HOST_MS - 10_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn adaptive_skew_uses_decayed_runnable_quantum_percentiles() {
        let mut skew = AdaptiveSkew::default();
        assert!((skew.fraction - 0.20).abs() < f64::EPSILON);
        skew.observe(0.25, 1_000);
        assert!((skew.fraction - 0.25).abs() < f64::EPSILON);
        skew.observe(0.0, 11_000);
        assert!((skew.weights[25] - 0.5).abs() < 0.000_001);
        for index in 1..=200 {
            skew.observe(0.0, 11_000 + index * 50);
        }
        assert!(skew.fraction < 0.25);
    }

    #[test]
    fn carried_clock_uses_the_more_conservative_rate() {
        assert_eq!(guest_tick_rate(0.20, 80.0, 80.0, false), 8_000_000);
        assert_eq!(guest_tick_rate(0.20, 80.0, 80.0, true), 8_000_000);
        assert_eq!(guest_tick_rate(0.20, 80.0, 100.0, true), 8_000_000);
        assert_eq!(guest_tick_rate(0.20, 80.0, 160.0, true), 5_000_000);
        assert_eq!(guest_tick_rate(0.20, 80.0, 160.0, false), 8_000_000);
    }

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
            for guest_tick_rate in [8_000_000, 9_999_999, 10_000_000] {
                for deadline in [1, 2, 997, 10_001] {
                    let quantum = ActiveQuantum {
                        start_guest_ticks: 17_300_000_000_000_000,
                        cycles_per_host_second: rate,
                        guest_ticks_per_host_second: guest_tick_rate,
                        applied_guest_clock_skew: 0.0,
                        carried_guest_ticks: 0,
                        budget_cycles: i32::MAX as u32,
                        consumed_cycles: 123,
                        stalled_cpu_runs: 0,
                        cpu_runs: 0,
                        timer_reprogramming_exits: 0,
                        timer_intervals_guest_ticks: Vec::new(),
                        record_timer_interval: false,
                        wfi_wake_delay_guest_ticks: 0,
                        terminal: None,
                    };
                    let target = quantum.consumed_cycles
                        + u64::from(quantum.cycles_until_deadline(deadline));
                    let offset = (u128::from(quantum.consumed_cycles)
                        * u128::from(guest_tick_rate)
                        / u128::from(rate))
                        + u128::from(deadline);
                    assert!(
                        u128::from(target) * u128::from(guest_tick_rate) / u128::from(rate)
                            >= offset
                    );
                    assert!(
                        u128::from(target - 1) * u128::from(guest_tick_rate) / u128::from(rate)
                            < offset
                    );
                }
            }
        }
    }

    #[test]
    fn skew_slows_guest_time_without_reducing_the_cycle_budget() {
        let mut quantum = ActiveQuantum {
            start_guest_ticks: 10_000_000_000,
            cycles_per_host_second: 300_000_000,
            guest_ticks_per_host_second: 8_000_000,
            applied_guest_clock_skew: 0.20,
            carried_guest_ticks: 0,
            budget_cycles: 3_000_000,
            consumed_cycles: 0,
            stalled_cpu_runs: 0,
            cpu_runs: 1,
            timer_reprogramming_exits: 0,
            timer_intervals_guest_ticks: Vec::new(),
            record_timer_interval: false,
            wfi_wake_delay_guest_ticks: 0,
            terminal: Some(QuantumOutcome::BudgetReached),
        };
        assert_eq!(quantum.budget_cycles, 3_000_000);
        assert_eq!(quantum.guest_ticks(), 10_000_000_000);
        quantum.consumed_cycles = quantum.budget_cycles.into();
        assert_eq!(quantum.guest_ticks(), 10_000_080_000);
    }

    #[test]
    fn whole_quantum_samples_replace_guess_and_decay_previous_measurements() {
        let mut rate = RateEstimate::default();
        rate.observe(100_000, 10.0);
        assert!((rate.emulated_cycles_per_host_second - 10_000_000.0).abs() < 0.001);
        rate.observe(50_000, 10.0);
        assert!(rate.emulated_cycles_per_host_second < 10_000_000.0);
        assert!(rate.emulated_cycles_per_host_second > 5_000_000.0);
        let measured = rate.emulated_cycles_per_host_second;
        rate.observe(0, 100.0);
        assert!((rate.emulated_cycles_per_host_second - measured).abs() < 0.001);
    }
}
