import { rulerGuides } from "./ruler-guides-browser.mjs";
import assert from "node:assert/strict";

export async function navigationRegressions(page, reference, check, helpers) {
    const { fixtureNode, isolatedDocument, importFixture, documentState, state, clickWorld, clickNative, moveWorld } = helpers;
    await importFixture(page, isolatedDocument(reference, [
        fixtureNode(reference, 2, { x: 20, y: 20, width: 180, height: 100 }),
        fixtureNode(reference, 3, { x: 480, y: 160, width: 120, height: 120 }),
    ], "Canvas navigation"));
    await clickWorld(page, 110, 70);
    const document = await documentState(page);
    const initial = await state(page);
    assert.deepEqual(initial.selection, [2]);
    await moveWorld(page, 540, 220);
    await page.keyboard.down("Alt");
    try {
        await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).measurementTarget === 3);
        const measured = await state(page);
        assert.deepEqual(measured.selection, [2]);
        assert.equal(measured.historyLength, initial.historyLength);
    } finally { await page.keyboard.up("Alt"); }
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).measurementTarget === null);
    assert.deepEqual(await documentState(page), document);
    check("Alt at a stationary pointer measures the hovered layer, then releases without changing artwork, selection or history");

    const waitZoom = zoom => page.waitForFunction(expected =>
        JSON.parse(globalThis.ferroDesign.ferroDesignState()).zoom === expected, zoom, { timeout: 10000 });

    await page.keyboard.press("Shift+0");
    await waitZoom(1);
    const actual = await state(page);
    const host = actual.chrome.CanvasHost;
    const pointer = { x: Math.floor(host.width * 0.42), y: Math.floor(host.height * 0.4) };
    const screen = { x: host.x + pointer.x, y: host.y + pointer.y };
    await page.mouse.move(screen.x, screen.y);
    await page.keyboard.down("Control");
    try { await page.mouse.wheel(0, -100); } finally { await page.keyboard.up("Control"); }
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).zoom > 1);
    const zoomed = await state(page);
    const near = (a, b) => assert.ok(Math.abs(a - b) < 0.01, a + " must match " + b);
    near((pointer.x - zoomed.pan.x) / zoomed.zoom, pointer.x - actual.pan.x);
    near((pointer.y - zoomed.pan.y) / zoomed.zoom, pointer.y - actual.pan.y);
    await page.mouse.wheel(50, 100);
    await page.waitForFunction(previous => JSON.parse(globalThis.ferroDesign.ferroDesignState()).pan.y !== previous, zoomed.pan.y);
    const scrolled = await state(page);
    assert.equal(scrolled.zoom, zoomed.zoom);
    assert.notEqual(scrolled.pan.x, zoomed.pan.x, "Horizontal scroll pans the canvas");
    check("Actual size and modifier-wheel zoom preserve the pointer's world position; unmodified two-axis wheel input pans");

    await page.keyboard.press("r");
    await page.keyboard.down("Space");
    try {
        await page.mouse.down();
        await page.mouse.move(screen.x + 55, screen.y - 28, { steps: 4 });
        await page.mouse.up();
    } finally { await page.keyboard.up("Space"); }
    const panned = await state(page);
    near(panned.pan.x - scrolled.pan.x, 55);
    near(panned.pan.y - scrolled.pan.y, -28);
    assert.equal(panned.tool, "Rectangle");
    assert.equal(panned.zoomLabel, scrolled.zoomLabel);
    assert.deepEqual(panned.selection, [2]);
    check("Space-drag pans in screen pixels and restores the rectangle tool without drawing, selecting or changing zoom");

    await clickNative(page, "ZoomValueButton");
    await page.keyboard.type("37.5");
    await page.keyboard.press("Enter");
    await waitZoom(0.375);
    assert.equal((await state(page)).zoomLabel, "38%");
    await page.keyboard.press("Shift+2");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).zoom > 0.375);
    assert.deepEqual((await state(page)).selection, [2]);
    assert.equal((await state(page)).historyLength, initial.historyLength);
    assert.deepEqual(await documentState(page), document);
    check("The native zoom flyout applies fractional percentages and returns focus to the canvas for Shift+2; navigation preserves document and history");

    const normal = await state(page);
    const waitUi = (mode, left, right, dock) => page.waitForFunction(expected => {
        const view = JSON.parse(globalThis.ferroDesign.ferroDesignState()), c = view.chrome;
        // Visibility changes synchronously. The next layout pass moves the
        // canvas and updates its bounds, which subsequent pointer input uses.
        const desktop = window.innerWidth >= 970;
        const origin = desktop && expected.left ? 296 : 0;
        const rightPanel = desktop && expected.right ? 288 : 0;
        const width = window.innerWidth - origin - rightPanel;
        return view.workspaceUi === expected.mode && c.CanvasHost.arrangeValid &&
            c.CanvasHost.x === origin && c.CanvasHost.width === width && view.canvas.width === width &&
            c.LeftPanel.visible === expected.left && c.RightPanel.visible === expected.right &&
            c.Toolbar.visible === expected.dock;
    }, { mode, left, right, dock });
    await clickNative(page, "MinimizeUiButton");
    await waitUi("minimized", false, true, true);
    const minimal = await state(page);
    near(minimal.chrome.CanvasHost.x + minimal.pan.x, normal.chrome.CanvasHost.x + normal.pan.x);
    assert.equal(minimal.zoom, normal.zoom);
    await page.keyboard.press("Control+Backslash");
    await waitUi("hidden", false, false, false);
    const hidden = await state(page);
    assert.equal(hidden.chrome.CanvasHost.width, page.viewportSize().width);
    assert.deepEqual(hidden.selection, [2]);
    await page.keyboard.press("F6");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.RestoreUiButton.focused);
    await page.keyboard.press("Enter");
    await waitUi("minimized", false, true, true);
    await page.keyboard.press("Escape");
    await waitUi("minimized", false, false, true);
    await page.keyboard.press("v");
    await clickWorld(page, 110, 70);
    await waitUi("minimized", false, true, true);
    assert.deepEqual((await state(page)).selection, [2]);
    await page.keyboard.press("Control+Shift+Backslash");
    await waitUi("expanded", true, true, true);
    const restored = await state(page);
    near(restored.pan.x, normal.pan.x);
    near(restored.pan.y, normal.pan.y);
    assert.deepEqual(await documentState(page), document);
    assert.equal(restored.historyLength, normal.historyLength);
    check("Minimize keeps the toolbar, properties follow selection, Hide leaves an editable canvas, and restoring the UI preserves the camera and undo history");

    const size = page.viewportSize();
    await page.setViewportSize({ width: 320, height: 568 });
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.CompactLayers.visible);
    await clickNative(page, "CompactLayers");
    await clickNative(page, "MinimizeUiButton");
    await waitUi("minimized", false, false, true);
    const phone = await state(page);
    assert.ok(phone.chrome.MinimizedInspectorButton.width >= 44 && phone.chrome.RestoreUiButton.height >= 44);
    await clickNative(page, "MinimizedInspectorButton");
    await waitUi("minimized", false, true, false);
    await clickNative(page, "MinimizedInspectorButton");
    await waitUi("minimized", false, false, true);
    await clickNative(page, "RestoreUiButton");
    await waitUi("expanded", true, false, false);
    await page.setViewportSize(size);
    await waitUi("expanded", true, true, true);
    assert.deepEqual(await documentState(page), document);
    check("On a 320-pixel phone, native recovery controls inspect a selection, return to the canvas, and reopen the previous drawer");
    await rulerGuides(page, reference, check, helpers);
}
