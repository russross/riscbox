// Each separator resizes only its two adjacent panes, preserving their combined size.
export function setupLayout() {
    for (const gutter of document.querySelectorAll(".gutter")) {
        const vertical = gutter.id === "bottom-split";
        const before = vertical ? document.getElementById("panes") : gutter.previousElementSibling;
        const after = gutter.nextElementSibling;
        const size = node => node.getBoundingClientRect()[vertical ? "height" : "width"];
        function resize(delta, first = size(before), second = size(after)) {
            const minimum = vertical ? 40 : 0;
            const next = Math.max(minimum, Math.min(first + second - minimum, first + delta));
            if (vertical) {
                before.style.flex = `1 1 0`;
                after.style.flex = `0 0 ${first + second - next}px`;
            } else {
                // Pixel weights replace all percentage weights before the first drag.
                const panes = [...before.parentElement.querySelectorAll(":scope > section")];
                const weights = panes.map(size);
                panes.forEach((pane, index) => { pane.style.flex = `${weights[index]} 1 0`; });
                before.style.flex = `${next} 1 0`;
                after.style.flex = `${first + second - next} 1 0`;
            }
            gutter.setAttribute("aria-valuenow", String(Math.round(next)));
        }
        gutter.addEventListener("pointerdown", event => {
            if (event.button !== 0) return;
            event.preventDefault();
            gutter.setPointerCapture(event.pointerId);
            const start = vertical ? event.clientY : event.clientX;
            const first = size(before), second = size(after);
            document.body.classList.add("dragging");
            const move = event => resize((vertical ? event.clientY : event.clientX) - start, first, second);
            const stop = () => {
                document.body.classList.remove("dragging");
                gutter.removeEventListener("pointermove", move);
                gutter.removeEventListener("lostpointercapture", stop);
            };
            gutter.addEventListener("pointermove", move);
            gutter.addEventListener("lostpointercapture", stop);
        });
        gutter.addEventListener("keydown", event => {
            const direction = vertical ? ["ArrowUp", "ArrowDown"] : ["ArrowLeft", "ArrowRight"];
            if (!direction.includes(event.key)) return;
            event.preventDefault();
            resize(event.key === direction[0] ? -20 : 20);
        });
    }
}
