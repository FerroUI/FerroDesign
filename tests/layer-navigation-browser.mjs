import assert from "node:assert/strict";

export async function layerKeyboardNavigation(page, reference, check, helpers) {
    const { fixtureNode, isolatedDocument, importFixture, clickNative, state, documentState } = helpers;
    const row = (id, name, parent = 1, children = []) => fixtureNode(reference, id, {
        kind: children.length ? "group" : "rectangle", name, parent, children,
        x: 10, y: 10, width: 30, height: 30,
    });
    const rest = Array.from({ length: 90 }, (_, i) => row(20 + i, "Line " + i));
    await importFixture(page, isolatedDocument(reference, [
        ...rest, row(2, "Other", 1, [3]), row(3, "Unrelated", 2),
        row(4, "Header", 1, [5]), row(5, "Primary title", 4),
        row(6, "Sidebar", 1, [7]), row(7, "Secondary title", 6),
    ], "Layers keyboard navigation"));
    const original = await documentState(page);
    const baseline = await state(page);
    const expectRow = (id, selection) => page.waitForFunction(({ id, selection }) => {
        const view = JSON.parse(globalThis.ferroDesign.ferroDesignState());
        return view.chrome["LayerRow" + id]?.focused === true &&
            JSON.stringify(view.selection) === JSON.stringify(selection);
    }, { id, selection });
    const press = async (key, id, selection) => {
        await page.keyboard.press(key);
        await expectRow(id, selection);
    };
    await clickNative(page, "CollapseLayersButton");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).layerRowCount === 96);
    await clickNative(page, "LayerRow7", "LayersViewport");
    await expectRow(7, [7]);
    await press("Control+ArrowDown", 4, [7]);
    await press("Control+ArrowRight", 5, [7]);
    await press("Control+Space", 5, [7, 5]);
    await clickNative(page, "CollapseLayersButton");
    await page.waitForFunction(() => {
        const view = JSON.parse(globalThis.ferroDesign.ferroDesignState());
        return view.layerRowCount === 95 && view.chrome.LayerRow5?.visible &&
            view.chrome.LayerRow7?.visible && !view.chrome.LayerRow3?.visible;
    });
    await clickNative(page, "LayerRow7", "LayersViewport");
    await press("Shift+ArrowDown", 4, [7, 4]);
    await press("Shift+ArrowUp", 7, [7]);
    await press("Control+End", 20, [7]);
    const far = await state(page);
    assert.ok(far.realizedLayerRows < 70, "Jumping to a distant row keeps native realization bounded");
    const viewport = far.chrome.LayersViewport, end = far.chrome.LayerRow20;
    assert.ok(end.y + end.height > viewport.y && end.y < viewport.y + viewport.height,
        "The focused last row has actually scrolled into view");
    await press("Home", 6, [6]);
    await press("ArrowLeft", 6, [6]);
    await page.keyboard.press("Control+f");
    await page.keyboard.insertText("title");
    await page.waitForFunction(() => {
        const view = JSON.parse(globalThis.ferroDesign.ferroDesignState());
        return view.chrome.LayerSearch?.focused && view.layerRowCount === 4;
    });
    await press("ArrowDown", 7, [7]);
    await press("ArrowLeft", 6, [6]);
    await press("ArrowRight", 7, [7]);
    await page.keyboard.press("Control+f");
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => {
        const view = JSON.parse(globalThis.ferroDesign.ferroDesignState());
        return view.chrome.LayerSearch.focused && !view.chrome.ClearLayerSearch.visible &&
            view.layerRowCount === 94 && !view.chrome.LayerRow7?.visible;
    });
    await page.keyboard.press("Escape");
    assert.deepEqual(await documentState(page), original);
    assert.equal((await state(page)).historyLength, baseline.historyLength);
    check("Layers keeps both selected paths open, moves real focus across virtualized rows, extends and toggles ranges, and traverses filtered branches without changing disclosure or undo");
}
