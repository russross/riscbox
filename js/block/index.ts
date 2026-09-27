/** Host-owned sector storage connected to one Riscbox VirtIO block device. */
export interface BlockProvider {
    /** Returns exactly `length` bytes beginning at the 512-byte sector. */
    read(sector: bigint, length: number): Uint8Array | Promise<Uint8Array>;
    /** Persists whole-sector bytes from the guest. */
    write(sector: bigint, bytes: Uint8Array): void | Promise<void>;
    /** Retires pending interface work while preserving provider data. */
    reset(): void;
    /** Releases resources when the VM is destroyed. */
    close(): void;
}
