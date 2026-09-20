export const MAX_ETHERNET_FRAME_SIZE = 65_535;
export const NETWORK_SEND_HIGH_WATER = 1 << 20;

const INITIAL_RECONNECT_DELAY_MS = 250;
const MAX_RECONNECT_DELAY_MS = 10_000;
const SOCKET_OPEN = 1;
const SOCKET_CLOSING = 2;

export interface NetworkRuntime {
    networkInput(packet: Uint8Array): number;
    networkCarrier(up: boolean): number;
}

export interface WebSocketNetworkOptions {
    webSocketFactory?: (url: string) => WebSocket;
    onError?: (error: Error) => void;
    setTimeout?: typeof globalThis.setTimeout;
    clearTimeout?: typeof globalThis.clearTimeout;
}

export class WebSocketNetwork {
    readonly endpoint: string;

    private readonly webSocketFactory: (url: string) => WebSocket;
    private readonly onError: ((error: Error) => void) | undefined;
    private readonly scheduleTimeout: typeof globalThis.setTimeout;
    private readonly cancelTimeout: typeof globalThis.clearTimeout;
    private runtime: NetworkRuntime | undefined;
    private socket: WebSocket | undefined;
    private reconnectTimer: ReturnType<typeof globalThis.setTimeout> | undefined;
    private reconnectDelay = INITIAL_RECONNECT_DELAY_MS;
    private stopped = true;

    constructor(endpoint: string, options: WebSocketNetworkOptions = {}) {
        this.endpoint = endpoint;
        this.webSocketFactory = options.webSocketFactory ?? ((url) => new WebSocket(url));
        this.onError = options.onError;
        this.scheduleTimeout = options.setTimeout ?? globalThis.setTimeout.bind(globalThis);
        this.cancelTimeout = options.clearTimeout ?? globalThis.clearTimeout.bind(globalThis);
        this.transmit = this.transmit.bind(this);
    }

    attach(runtime: NetworkRuntime): void {
        if (this.runtime !== undefined)
            throw new Error("WebSocket network runtime is already attached");
        this.runtime = runtime;
        runtime.networkCarrier(false);
    }

    connect(): void {
        if (this.runtime === undefined)
            throw new Error("WebSocket network runtime is not attached");
        if (!this.stopped || this.socket !== undefined)
            return;
        this.stopped = false;
        this.openSocket();
    }

    close(): void {
        this.stopped = true;
        if (this.reconnectTimer !== undefined) {
            this.cancelTimeout(this.reconnectTimer);
            this.reconnectTimer = undefined;
        }
        const socket = this.socket;
        this.socket = undefined;
        this.runtime?.networkCarrier(false);
        if (socket !== undefined && socket.readyState < SOCKET_CLOSING)
            socket.close(1000, "Riscbox network closed");
    }

    transmit(packet: Uint8Array): boolean {
        const socket = this.socket;
        if (packet.length === 0 || packet.length > MAX_ETHERNET_FRAME_SIZE)
            return false;
        if (socket === undefined || socket.readyState !== SOCKET_OPEN)
            return false;
        if (socket.bufferedAmount + packet.length > NETWORK_SEND_HIGH_WATER)
            return false;
        socket.send(packet.slice());
        return true;
    }

    private openSocket(): void {
        let socket: WebSocket;
        try {
            socket = this.webSocketFactory(this.endpoint);
        } catch (error) {
            this.reportError(error, "could not create network WebSocket");
            this.scheduleReconnect();
            return;
        }
        this.socket = socket;
        socket.binaryType = "arraybuffer";
        socket.onopen = () => {
            if (this.socket !== socket || this.stopped)
                return;
            this.reconnectDelay = INITIAL_RECONNECT_DELAY_MS;
            this.runtime?.networkCarrier(true);
        };
        socket.onmessage = (event: MessageEvent<unknown>) => {
            if (this.socket !== socket || this.stopped)
                return;
            if (!(event.data instanceof ArrayBuffer)) {
                this.protocolError(socket, "network WebSocket sent a non-binary message");
                return;
            }
            const packet = new Uint8Array(event.data);
            if (packet.length === 0 || packet.length > MAX_ETHERNET_FRAME_SIZE) {
                this.protocolError(socket, "network WebSocket sent an invalid frame size");
                return;
            }
            const result = this.runtime?.networkInput(packet);
            if (result !== undefined && result < 0)
                this.reportError(new Error("Riscbox rejected network ingress"));
        };
        socket.onerror = () => {
            if (this.socket !== socket || this.stopped)
                return;
            this.runtime?.networkCarrier(false);
            this.reportError(new Error("network WebSocket failed"));
        };
        socket.onclose = () => {
            if (this.socket !== socket)
                return;
            this.socket = undefined;
            this.runtime?.networkCarrier(false);
            this.scheduleReconnect();
        };
    }

    private protocolError(socket: WebSocket, message: string): void {
        this.reportError(new Error(message));
        this.runtime?.networkCarrier(false);
        socket.close(1003, message);
    }

    private scheduleReconnect(): void {
        if (this.stopped || this.reconnectTimer !== undefined)
            return;
        const delay = this.reconnectDelay;
        this.reconnectDelay = Math.min(this.reconnectDelay * 2, MAX_RECONNECT_DELAY_MS);
        this.reconnectTimer = this.scheduleTimeout(() => {
            this.reconnectTimer = undefined;
            if (!this.stopped)
                this.openSocket();
        }, delay);
    }

    private reportError(error: unknown, fallback?: string): void {
        if (error instanceof Error)
            this.onError?.(error);
        else
            this.onError?.(new Error(fallback ?? String(error)));
    }
}
