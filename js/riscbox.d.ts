export interface HttpDrive {
    readonly file: string;
    readonly device?: string;
}
export interface ArrayDrive {
    readonly capacity_sectors?: bigint | number | string;
    readonly bytes?: Uint8Array;
}
export interface FilesystemConfig {
    readonly server: string;
    readonly tag: string;
}
export interface VmConfig {
    readonly version: 1;
    readonly machine: "riscv64";
    readonly memory_size: number;
    readonly bios?: string;
    readonly kernel?: string;
    readonly initrd?: string;
    readonly cmdline?: string;
    readonly console?: "uart" | "virtio";
    readonly uart_output?: boolean;
    readonly rtc_local_time?: boolean;
    readonly bios_address?: number | string;
    readonly kernel_address?: number | string;
    readonly initrd_address?: number | string;
    readonly fdt_address?: number | string;
    readonly display0?: { readonly device: "simplefb"; readonly width: number; readonly height: number };
    readonly input_device?: "virtio";
    readonly eth0?: { readonly driver: "user" };
    readonly drive0?: HttpDrive | ArrayDrive;
    readonly drive1?: HttpDrive | ArrayDrive;
    readonly drive2?: HttpDrive | ArrayDrive;
    readonly drive3?: HttpDrive | ArrayDrive;
    readonly fs0?: FilesystemConfig;
    readonly fs1?: FilesystemConfig;
    readonly fs2?: FilesystemConfig;
    readonly fs3?: FilesystemConfig;
}
export interface BlockFetchRequest {
    readonly disk: number;
    readonly url: string;
    readonly cache: RequestCache;
}
export interface RiscboxOptions {
    readonly fetch?: typeof fetch;
    readonly fetchBlock?: (request: BlockFetchRequest) => Promise<Uint8Array>;
    readonly targetQuantumMs?: number;
    readonly debugTiming?: boolean;
    readonly consoleWrite?: (text: string) => void;
    readonly consoleReset?: () => void;
    readonly onVmStarted?: () => void;
    readonly onVmHalted?: (cause: HaltCause) => void;
    readonly onVmReset?: (cause: ResetCause) => void;
    readonly onVmDestroyed?: () => void;
    readonly onError?: (error: unknown) => void;
    readonly networkWrite?: (bytes: Uint8Array) => void;
    readonly framebufferClear?: () => void;
    readonly framebufferRefresh?: (bytes: Uint8Array, geometry: { x: number; y: number; width: number; height: number; stride: number }) => void;
}
export type ConfigSource =
    | { readonly url: string | URL }
    | { readonly text: string; readonly baseUrl?: string | URL }
    | { readonly value: VmConfig; readonly baseUrl?: string | URL };
export interface BlockOverrides {
    readonly drive0?: HttpDrive | ArrayDrive;
    readonly drive1?: HttpDrive | ArrayDrive;
    readonly drive2?: HttpDrive | ArrayDrive;
    readonly drive3?: HttpDrive | ArrayDrive;
}
export interface PreparationOptions extends RiscboxOptions {
    readonly config: ConfigSource;
    readonly wasmUrl?: string | URL;
    readonly blocks?: BlockOverrides;
    readonly ramMiB?: number;
    readonly width?: number;
    readonly height?: number;
    readonly hasNetwork?: boolean;
    readonly commandLine?: string;
    readonly signal?: AbortSignal;
}
export type VmState = "halted" | "running" | "destroyed";
export type HaltCause = "guest-poweroff" | "host-halt" | "guest-failure";
export type ResetCause = "guest-reboot" | "host-reset" | "host-boot";

// The client boundary exposes owned storage and automatic VM execution.
export declare class Riscbox {
    private constructor();
    static readonly FilesystemError: typeof FilesystemError;
    static readonly BlockError: typeof BlockError;
    readonly state: VmState;
    readonly started: boolean;
    static prepare(options: PreparationOptions): Promise<Riscbox>;
    filesystem(name: string): Filesystem;
    block(index: number): BlockDisk;
    boot(): Promise<void>;
    halt(): Promise<void>;
    reset(): Promise<void>;
    coldReset(): Promise<void>;
    destroy(): Promise<void>;
    requestShutdown(): Promise<void>;
    requestReboot(): Promise<void>;
    consoleInput(bytes: Uint8Array): number;
    consoleResize(columns: number, rows: number): number;
    keyEvent(down: boolean, code: number): number;
    pointerEvent(x: number, y: number, buttons: number): number;
    wheelEvent(delta: number): number;
    networkInput(bytes: Uint8Array): number;
    networkCarrier(up: boolean): number;
}
import type { BlockDisk, Filesystem, FilesystemError, BlockError } from "./storage";
