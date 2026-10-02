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
    readonly onVmHalted?: (cause: string) => void;
    readonly onVmReset?: (cause: string) => void;
    readonly onVmDestroyed?: () => void;
    readonly onError?: (error: unknown) => void;
    readonly networkWrite?: (bytes: Uint8Array) => void;
    readonly framebufferClear?: () => void;
    readonly framebufferRefresh?: (bytes: Uint8Array, geometry: { x: number; y: number; width: number; height: number; stride: number }) => void;
}
export declare class Riscbox implements StorageRuntime {
    readonly exports: StorageExports;
    readonly started: boolean;
    readonly filesystems: Map<number, Filesystem>;
    static instantiate(bytes: BufferSource | WebAssembly.Module, options?: RiscboxOptions): Promise<Riscbox>;
    static loadResolvedConfig(url: string, commandLine?: string, fetchRequest?: typeof fetch): Promise<VmConfig>;
    prepareResolved(config: VmConfig, ramMiB?: number, width?: number, height?: number, hasNetwork?: boolean): Promise<void>;
    prepareFromUrl(url: string, ramMiB?: number, commandLine?: string, width?: number, height?: number, hasNetwork?: boolean): Promise<void>;
    startResolved(config: VmConfig, ramMiB?: number, width?: number, height?: number, hasNetwork?: boolean): Promise<void>;
    startFromUrl(url: string, ramMiB?: number, commandLine?: string, width?: number, height?: number, hasNetwork?: boolean): Promise<void>;
    start(url: string, ramMiB: number, commandLine?: string, width?: number, height?: number, hasNetwork?: boolean): Promise<void>;
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
    runQuantum(): void;
    networkInput(bytes: Uint8Array): number;
    networkCarrier(up: boolean): number;
    withBytes<Value>(bytes: Uint8Array | string, call: (address: number, length: number) => Value): Value;
    bytes(address: number, length: number): Uint8Array;
    serviceStorage(): void;
    drainActions(): void;
    filesystemChanged(): void;
    reportFilesystemError(error: unknown): void;
}
import type { BlockDisk, Filesystem, StorageExports, StorageRuntime } from "./storage";
