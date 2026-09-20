import assert from "node:assert/strict";
import test from "node:test";

import {
    MAX_ETHERNET_FRAME_SIZE,
    NETWORK_SEND_HIGH_WATER,
    WebSocketNetwork,
} from "../build/js/network/index.js";

class FakeSocket {
    binaryType = "blob";
    bufferedAmount = 0;
    readyState = 0;
    onopen = null;
    onmessage = null;
    onerror = null;
    onclose = null;
    sent = [];
    closed = [];

    send(data) {
        this.sent.push(data);
    }

    close(code, reason) {
        this.readyState = 2;
        this.closed.push([code, reason]);
    }

    open() {
        this.readyState = 1;
        this.onopen?.({});
    }

    message(data) {
        this.onmessage?.({ data });
    }

    fail() {
        this.onerror?.({});
    }

    finishClose() {
        this.readyState = 3;
        this.onclose?.({});
    }
}

function harness() {
    const sockets = [];
    const carriers = [];
    const packets = [];
    const errors = [];
    const timers = [];
    const runtime = {
        networkCarrier(up) {
            carriers.push(up);
            return 0;
        },
        networkInput(packet) {
            packets.push(packet.slice());
            return 0;
        },
    };
    const network = new WebSocketNetwork("ws://origin/network", {
        webSocketFactory(url) {
            assert.equal(url, "ws://origin/network");
            const socket = new FakeSocket();
            sockets.push(socket);
            return socket;
        },
        onError(error) {
            errors.push(error.message);
        },
        setTimeout(callback, delay) {
            const timer = { callback, delay, canceled: false };
            timers.push(timer);
            return timer;
        },
        clearTimeout(timer) {
            timer.canceled = true;
        },
    });
    return { network, runtime, sockets, carriers, packets, errors, timers };
}

test("open, receive, transmit, and close follow carrier state", () => {
    const state = harness();
    state.network.attach(state.runtime);
    assert.deepEqual(state.carriers, [false]);
    state.network.connect();
    const socket = state.sockets[0];
    assert.equal(socket.binaryType, "arraybuffer");
    socket.open();
    assert.deepEqual(state.carriers, [false, true]);

    const inbound = Uint8Array.of(1, 2, 3);
    socket.message(inbound.buffer);
    inbound[0] = 9;
    assert.deepEqual(state.packets, [Uint8Array.of(1, 2, 3)]);

    const outbound = Uint8Array.of(4, 5, 6);
    assert.equal(state.network.transmit(outbound), true);
    outbound[0] = 9;
    assert.deepEqual(socket.sent, [Uint8Array.of(4, 5, 6)]);

    state.network.close();
    assert.deepEqual(state.carriers, [false, true, false]);
    assert.equal(socket.closed[0][0], 1000);
});

test("invalid messages and send pressure drop without unbounded buffering", () => {
    const state = harness();
    state.network.attach(state.runtime);
    state.network.connect();
    const socket = state.sockets[0];
    socket.open();
    socket.bufferedAmount = NETWORK_SEND_HIGH_WATER;
    assert.equal(state.network.transmit(Uint8Array.of(1)), false);
    assert.equal(state.network.transmit(new Uint8Array()), false);
    assert.equal(
        state.network.transmit(new Uint8Array(MAX_ETHERNET_FRAME_SIZE + 1)),
        false,
    );

    socket.message("text");
    assert.equal(socket.closed[0][0], 1003);
    assert.match(state.errors[0], /non-binary/);
});

test("errors lower carrier and reconnect with bounded backoff", () => {
    const state = harness();
    state.network.attach(state.runtime);
    state.network.connect();
    state.sockets[0].open();
    state.sockets[0].fail();
    assert.deepEqual(state.carriers, [false, true, false]);
    state.sockets[0].finishClose();
    assert.equal(state.timers[0].delay, 250);
    state.timers[0].callback();
    state.sockets[1].finishClose();
    assert.equal(state.timers[1].delay, 500);
    state.network.close();
    assert.equal(state.timers[1].canceled, true);
});

test("independent adapters never share sockets or runtime state", () => {
    const first = harness();
    const second = harness();
    first.network.attach(first.runtime);
    second.network.attach(second.runtime);
    first.network.connect();
    second.network.connect();
    first.sockets[0].open();
    first.sockets[0].message(Uint8Array.of(7).buffer);
    assert.deepEqual(first.packets, [Uint8Array.of(7)]);
    assert.deepEqual(second.packets, []);
    assert.notEqual(first.sockets[0], second.sockets[0]);
});
