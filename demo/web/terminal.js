import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";

// The terminal owns one parser and fits whole cells inside the available pane.
export function createTerminal(host, onData, onResize) {
    const terminal = new Terminal({
        fontFamily: '"Latin Modern Mono", monospace',
        fontSize: Number.parseFloat(getComputedStyle(document.documentElement).fontSize),
        lineHeight: 1, cursorBlink: true, scrollback: 64 * 1024,
        smoothScrollDuration: 0, customGlyphs: true,
        windowOptions: { getWinSizePixels: true, getCellSizePixels: true },
        theme: {
            background: "#000000", foreground: "#c0c0c0", cursor: "#c0c0c0", cursorAccent: "#000000",
            black: "#000000", red: "#ff0000", green: "#00ff00", yellow: "#ffff00",
            blue: "#0000ff", magenta: "#ff00ff", cyan: "#00ffff", white: "#ffffff",
            brightBlack: "#808080", brightRed: "#ff8080", brightGreen: "#80ff80", brightYellow: "#ffff80",
            brightBlue: "#8080ff", brightMagenta: "#ff80ff", brightCyan: "#80ffff", brightWhite: "#ffffff",
        },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);
    terminal.onData(onData);
    terminal.onBinary(text => onData(Uint8Array.from(text, character => character.charCodeAt(0))));
    terminal.onResize(onResize);
    terminal.textarea.setAttribute("aria-label", "Virtual machine console");

    // Clipboard paste uses xterm's bracketed-paste handling and strips embedded escapes.
    host.addEventListener("paste", event => {
        if (!event.clipboardData) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        terminal.clearSelection();
        terminal.scrollToBottom();
        const text = event.clipboardData.getData("text/plain");
        terminal.paste(terminal.modes.bracketedPasteMode ? text.replace(/\x1b/g, "") : text);
    }, { capture: true });
    const renderer = new WebglAddon();
    renderer.onContextLoss(() => {
        renderer.dispose();
        console.warn("Terminal WebGL context lost; using the DOM renderer");
    });
    try { terminal.loadAddon(renderer); }
    catch (error) { console.warn("Terminal WebGL unavailable; using the DOM renderer", error); }

    // Resize and font readiness both recalculate rows and columns, then notify the guest.
    const resize = () => { if (host.clientWidth > 8 && host.clientHeight > 8) fit.fit(); };
    new ResizeObserver(resize).observe(host);
    document.fonts.ready.then(resize);
    terminal.readText = async () => {
        await new Promise(resolve => terminal.write("", resolve));
        const buffer = terminal.buffer.active;
        return Array.from({ length: buffer.length }, (_, row) => buffer.getLine(row)?.translateToString(true) ?? "").join("\n");
    };
    return terminal;
}
