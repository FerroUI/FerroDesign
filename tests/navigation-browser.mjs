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
}
