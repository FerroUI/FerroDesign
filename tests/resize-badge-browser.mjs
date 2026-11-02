import assert from "node:assert/strict";
import { PNG } from "pngjs";

// Pan with the actual native pointer so that the ordinary below-selection
// position lies under the live dock. Neither device width nor zoom is fixed.
export async function resizeBadgeClearsDock(page, { state, moveWorld, waitForNativeFrame, captureNativeScreenshot }, mode) {
    const initial = await state(page);
    const bounds = initial.selectionBounds, dock = initial.chrome.Toolbar;
    const centerX = bounds.x + bounds.width / 2, bottom = bounds.y + bounds.height;
    const desired = { x: dock.x + dock.width / 2, y: dock.y - 12 };
    await moveWorld(page, centerX, bottom);
    await page.mouse.down({ button: "middle" });
    try { await page.mouse.move(desired.x, desired.y); }
    finally { await page.mouse.up({ button: "middle" }); }
    await page.waitForFunction(({ centerX, bottom, desired }) => {
        const v = JSON.parse(globalThis.ferroDesign.ferroDesignState());
        const canvas = v.chrome.CanvasHost;
        return Math.abs(canvas.x + v.pan.x + v.zoom * centerX - desired.x) <= 2 &&
            Math.abs(canvas.y + v.pan.y + v.zoom * bottom - desired.y) <= 2;
    }, { centerX, bottom, desired });

    const right = bounds.x + bounds.width, midY = bounds.y + bounds.height / 2;
    await moveWorld(page, right, midY);
    await page.mouse.down();
    try {
        const before = await state(page);
        await moveWorld(page, right + 18 / before.zoom, midY);
        await page.waitForFunction(width => JSON.parse(globalThis.ferroDesign.ferroDesignState()).selectionBounds.width > width, bounds.width);
        await waitForNativeFrame(page, before.renderedFrames, "Shared size badge at the tool dock");
        const bytes = await captureNativeScreenshot(mode + "-resize-readout-dock", { controls: ["CanvasHost", "Toolbar"] });
        const v = await state(page), box = v.selectionBounds, toolbar = v.chrome.Toolbar;
        const x = Math.round(v.chrome.CanvasHost.x + v.pan.x + v.zoom * (box.x + box.width / 2));
        const png = PNG.sync.read(bytes);
        let paintedRows = 0, lastRow = -1;
        const bottomY = Math.round(v.chrome.CanvasHost.y + v.pan.y + v.zoom * (box.y + box.height));
        for (let y = Math.max(0, bottomY - 52); y < Math.min(png.height, bottomY + 34); y++) {
            let pixels = 0;
            for (let px = Math.max(0, x - 72); px < Math.min(png.width, x + 72); px++) {
                const i = (y * png.width + px) * 4, data = png.data;
                if (Math.abs(data[i] - 9) <= 3 && Math.abs(data[i + 1] - 109) <= 4 && Math.abs(data[i + 2] - 174) <= 5) pixels++;
            }
            if (pixels >= 9) { paintedRows++; lastRow = y; }
        }
        assert.ok(paintedRows >= 15, "The native size badge must show its full height; painted rows=" + paintedRows);
        assert.ok(lastRow < toolbar.y - 3, "The size badge must clear the measured tool dock");
        await page.keyboard.press("Escape");
    } finally { await page.mouse.up(); }
    assert.deepEqual((await state(page)).selectionBounds, bounds);
}
