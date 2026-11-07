import assert from "node:assert/strict";

export async function frameResizeConstraints(page, reference, check, helpers) {
    const { fixtureNode, isolatedDocument, importFixture, clickNative, clickWorld, moveWorld, documentState, state } = helpers;
    const frame = fixtureNode(reference, 2, {
        kind: "frame", name: "Viewport", parent: 1, children: [3], x: 40, y: 60, width: 200, height: 130,
        rotation: 0, corner_radius: 0, corner_radii: null,
    });
    const child = fixtureNode(reference, 3, {
        kind: "rectangle", name: "Inset", parent: 2, children: [], x: 80, y: 30, width: 40, height: 28,
        constraints: { horizontal: "stretch", vertical: "start" },
    });
    await importFixture(page, isolatedDocument(reference, [frame, child], "Frame resize constraint override"));
    await clickNative(page, "SelectTool");
    await clickWorld(page, 185, 165);
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).selection.join() === "2");
    const initial = await documentState(page), history = (await state(page)).historyLength;
    await moveWorld(page, 40, 125);
    await page.mouse.down();
    await moveWorld(page, 100, 125);
    const width = value => page.waitForFunction(value => {
        return JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[3].width === value;
    }, value);
    await width(0);
    await page.keyboard.down("Control");
    try {
        await width(40);
        const { nodes } = await documentState(page);
        assert.ok(Math.abs(nodes[2].x + nodes[3].x - 120) < 1e-7, "Held Control keeps the child's canvas position");
        assert.equal((await state(page)).historyLength, history);
        await page.mouse.up();
    } finally { await page.keyboard.up("Control"); }
    assert.equal((await state(page)).historyLength, history + 1);
    await page.keyboard.press("Control+z");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[2].width === 200);
    assert.deepEqual(await documentState(page), initial);
    check("Control at a stationary left-edge drag restores child geometry from before the constrained preview and commits one reversible frame resize");
}
