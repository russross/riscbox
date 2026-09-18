(function (root) {
    "use strict";

    const encoder = new TextEncoder();
    const decoder = new TextDecoder();

    class Riscbox {
        constructor(exports, options = {}) {
            if (!(exports.memory instanceof WebAssembly.Memory))
                throw new TypeError("Riscbox WASM must export memory");
            this.exports = exports;
            this.options = options;
        }

        static hostImports(options = {}) {
            let runtime = null;
            const bytes = (ptr, len) => runtime.bytes(ptr, len);
            const imports = {
                vm_started() {
                    options.onVmStarted?.();
                },
                schedule(delay) {
                    const milliseconds = Math.max(0, delay | 0);
                    if (options.schedule)
                        options.schedule(milliseconds);
                    else
                        setTimeout(() => runtime.exports.riscbox_run?.(), milliseconds);
                },
                console_write(ptr, len) {
                    const data = bytes(ptr, len);
                    options.consoleWrite?.(decoder.decode(data));
                },
                framebuffer_refresh(ptr, x, y, width, height, stride) {
                    options.framebufferRefresh?.(
                        bytes(ptr, stride * height),
                        { x, y, width, height, stride },
                    );
                },
                network_write(ptr, len) {
                    options.networkWrite?.(bytes(ptr, len));
                },
            };
            return {
                imports,
                attach(exports) {
                    runtime = new Riscbox(exports, options);
                    return runtime;
                },
            };
        }

        static async instantiate(source, options = {}) {
            const host = Riscbox.hostImports(options);
            const result = await WebAssembly.instantiate(source, {
                riscbox_host: host.imports,
            });
            return host.attach(result.instance?.exports ?? result.exports);
        }

        bytes(ptr, len) {
            if (!Number.isInteger(ptr) || !Number.isInteger(len) || ptr < 0 || len < 0)
                throw new RangeError("invalid WASM byte range");
            const end = ptr + len;
            const memory = new Uint8Array(this.exports.memory.buffer);
            if (!Number.isSafeInteger(end) || end > memory.length)
                throw new RangeError("WASM byte range is outside memory");
            return memory.slice(ptr, end);
        }

        withBytes(value, callback) {
            const data = typeof value === "string" ? encoder.encode(value) : value;
            if (!(data instanceof Uint8Array))
                throw new TypeError("expected a string or Uint8Array");
            if (data.length === 0)
                return callback(0, 0);
            const ptr = this.exports.riscbox_alloc(data.length);
            if (!Number.isInteger(ptr) || ptr <= 0)
                throw new Error("Riscbox WASM allocation failed");
            try {
                new Uint8Array(this.exports.memory.buffer, ptr, data.length).set(data);
                return callback(ptr, data.length);
            } finally {
                this.exports.riscbox_free(ptr, data.length);
            }
        }

        start(configUrl, ramMiB, commandLine = "", password = "", width = 0,
              height = 0, hasNetwork = false) {
            return this.withBytes(configUrl, (urlPtr, urlLen) =>
                this.withBytes(commandLine, (commandPtr, commandLen) =>
                    this.withBytes(password ?? "", (passwordPtr, passwordLen) =>
                        this.exports.riscbox_start(
                            urlPtr, urlLen, ramMiB, commandPtr, commandLen,
                            passwordPtr, passwordLen, width, height,
                            hasNetwork ? 1 : 0,
                        ))));
        }

        consoleInput(data) {
            return this.withBytes(data, (ptr, len) =>
                this.exports.riscbox_console_input(ptr, len));
        }

        consoleResize(width, height) {
            return this.exports.riscbox_console_resize(width, height);
        }

        keyEvent(isDown, keyCode) {
            return this.exports.riscbox_key_event(isDown ? 1 : 0, keyCode);
        }

        pointerEvent(x, y, buttons) {
            return this.exports.riscbox_pointer_event(x, y, buttons);
        }

        wheelEvent(delta) {
            return this.exports.riscbox_wheel_event(delta);
        }

        networkInput(packet) {
            return this.withBytes(packet, (ptr, len) =>
                this.exports.riscbox_network_input(ptr, len));
        }

        networkCarrier(up) {
            return this.exports.riscbox_network_carrier(up ? 1 : 0);
        }

        ccall(name, returnType, argumentTypes, args = []) {
            if (name !== "vm_start")
                throw new Error(`unsupported compatibility call: ${name}`);
            if (returnType !== null || argumentTypes.length !== 7 || args.length !== 7)
                throw new TypeError("vm_start compatibility signature mismatch");
            return this.start(...args);
        }
    }

    root.Riscbox = Riscbox;
    if (typeof module === "object" && module.exports)
        module.exports = { Riscbox };
}(globalThis));
