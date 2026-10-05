import { Riscbox, type Filesystem, type VmConfig } from "../build/js/riscbox";
import { WebSocketNetwork } from "../build/js/network/index";

// Compile a consumer's prepare/populate/boot flow against the deployable declarations.
export async function embed(wasmUrl: string, config: VmConfig): Promise<Riscbox> {
    const network = new WebSocketNetwork("wss://example.invalid/ethernet");
    const runtime = await Riscbox.prepare({
        wasmUrl, config: { value: config }, hasNetwork: true,
        consoleWrite: text => console.log(text),
        networkWrite: network.transmit,
        onVmHalted: cause => console.log(cause),
    });
    network.attach(runtime);
    const share: Filesystem = runtime.filesystem("workspace");
    share.writeFile("hello.c", "int main(void) { return 0; }\n", 1n);
    const unsubscribe = share.subscribe(change => console.log(change.path, change.origin));
    await runtime.boot();
    unsubscribe();
    return runtime;
}

// Recovery operates on the owned disk while halted and leaves the resident share alone.
export async function recover(runtime: Riscbox): Promise<void> {
    if (runtime.state === "running") await runtime.halt();
    await runtime.coldReset();
    const disk = runtime.block(0);
    const saved: Uint8Array = await disk.read(0n, 512);
    console.log(saved.byteLength, disk.capacitySectors);
    disk.reset();
    runtime.filesystem("workspace").reset();
    runtime.filesystem("workspace").clear();
    await runtime.boot();
}
