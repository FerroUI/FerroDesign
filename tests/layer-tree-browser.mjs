import assert from "node:assert/strict";

export async function layerTreeDrag(page, reference, check, helpers) {
    const { fixtureNode, isolatedDocument, importFixture, state, documentState } = helpers;
    const make = (id, kind, name, x) => fixtureNode(reference, id, {
        kind, name, x, y: 40, width: 120, height: 80, parent: 1, children: [],
    });
    await importFixture(page, isolatedDocument(reference, [
        make(2, "group", "Controls", 240), make(3, "rectangle", "Badge", 40),
        make(4, "rectangle", "Footer", 390),
    ], "Layer row drag"));
    await page.waitForFunction(() => {
        const chrome = JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome;
        return chrome.LayerRow2?.width > 0 && chrome.LayerRow3?.width > 0;
    });
    const before = await documentState(page);
    const baseline = await state(page);
    const point = row => [row.x + row.width / 2, row.y + row.height / 2];
    const start = point(baseline.chrome.LayerRow3);
    const end = point(baseline.chrome.LayerRow2);
    async function drag() {
        await page.mouse.move(...start);
        await page.mouse.down();
        await page.mouse.move(...end, { steps: 4 });
    }
    await drag();
    assert.deepEqual(await documentState(page), before);
    await page.keyboard.press("Escape");
    await page.mouse.up();
    assert.deepEqual(await documentState(page), before);
    assert.equal((await state(page)).historyLength, baseline.historyLength);
    await drag();
    await page.mouse.up();
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[3].parent === 2);
    const after = await documentState(page);
    assert.equal(after.nodes[3].x + after.nodes[2].x, before.nodes[3].x);
    assert.equal(after.nodes[3].y + after.nodes[2].y, before.nodes[3].y);
    assert.deepEqual(after.nodes[2].children, [3]);
    assert.deepEqual((await state(page)).selection, [3]);
    assert.equal((await state(page)).historyLength, baseline.historyLength + 1);
    await page.keyboard.press("Control+z");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[3].parent === 1);
    assert.deepEqual(await documentState(page), before);
    check("Dragging a native Layers row previews, cancels or nests in a group while preserving canvas placement and exact undo");
}
