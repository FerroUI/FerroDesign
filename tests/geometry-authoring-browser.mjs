import assert from "node:assert/strict";
import { PNG } from "pngjs";

export async function geometryAuthoringRegressions(page, reference, check, helpers, mode) {
    const { fixtureNode, isolatedDocument, importFixture, documentState, state,
        clickWorld, moveWorld, clickNative, frames, waitForNativeFrame, waitForNativeScene, captureNativeScreenshot } = helpers;
    const near = (a, b, label) => assert.ok(Math.abs(a - b) < 1e-7, label + ": " + a + " != " + b);
    const edit = async (name, value, container = "ModalViewport") => {
        await clickNative(page, name, container);
        await page.keyboard.press("Control+a");
        await page.keyboard.insertText(value);
        await page.keyboard.press("Enter");
        await frames(page);
    };
    const history = async (before, after, depth, label) => {
        assert.equal((await state(page)).historyLength, depth + 1, label + " must be one transaction");
        await page.keyboard.press("Control+z");
        await frames(page);
        assert.deepEqual(await documentState(page), before, label + " undo restores every field");
        await page.keyboard.press("Control+Shift+z");
        await frames(page);
        assert.deepEqual(await documentState(page), after, label + " redo restores every field");
    };
    const selectParent = async (id, label) => {
        const before = await state(page);
        await page.keyboard.press("Shift+Enter");
        await page.waitForFunction((id) => {
            const selection = JSON.parse(globalThis.ferroDesign.ferroDesignState()).selection;
            return selection.length === 1 && selection[0] === id;
        }, id);
        // Keyboard selection updates the model before the replacement inspector
        // owns the compositor's hit-test scene. Use the same publication
        // contract as clickWorld before sending input to its native fields.
        await waitForNativeFrame(page, before.renderedFrames, label);
        await waitForNativeScene(page, before.sceneFrames, label);
    };

    await importFixture(page, isolatedDocument(reference, [
        fixtureNode(reference, 2, { kind: "frame", name: "Affine parent", x: 40, y: 40,
            width: 240, height: 180, children: [3], fills: [], corner_radius: 0 }),
        fixtureNode(reference, 3, { kind: "group", parent: 2, name: "Editable group", x: 20, y: 15,
            width: 100, height: 60, children: [4], fills: [], corner_radius: 0 }),
        fixtureNode(reference, 4, { parent: 3, name: "Unflattened child", x: 7, y: 9,
            width: 23, height: 17, corner_radius: 0 }),
    ], "Native affine transform"));
    await clickWorld(page, 78, 72);
    assert.deepEqual((await state(page)).selection, [4]);
    await selectParent(3, "Affine parent selection");
    const before = await documentState(page), depth = (await state(page)).historyLength;
    await clickNative(page, "AffineButton", "InspectorViewport");
    await edit("AffineC", "0.5");
    const transformed = await documentState(page);
    near(transformed.nodes[3].affine_transform.c, 0.5, "Native horizontal shear");
    assert.deepEqual(transformed.nodes[4], before.nodes[4], "Matrix editing retains child geometry, properties and IDs");
    assert.deepEqual(transformed.nodes[2].children, [3]);
    assert.deepEqual(transformed.nodes[3].children, [4]);
    await captureNativeScreenshot(mode + "-native-affine-authoring", { controls: ["ModalPanel", "AffineA", "AffineC", "AffineX"] });
    await clickNative(page, "CloseModalButton");
    await history(before, transformed, depth, "Native affine field");
    check("Native affine matrix fields preserve editable child geometry and hierarchy, render the transform, and commit one complete undo/redo transaction");

    await clickNative(page, "AffineButton", "InspectorViewport");
    const invalidDepth = (await state(page)).historyLength;
    for (const [name, invalid, restored] of [["AffineC", "NaN", "0.5"], ["AffineA", "0", "1"]]) {
        await edit(name, invalid);
        const rejected = await state(page);
        assert.equal(rejected.chrome[name].text, invalid);
        assert.equal(rejected.chrome.AffineError.visible, true);
        assert.equal(rejected.historyLength, invalidDepth);
        assert.deepEqual(await documentState(page), transformed);
        await page.keyboard.press("Escape");
        await frames(page);
        assert.equal((await state(page)).chrome[name].text, restored);
    }
    await clickNative(page, "CloseModalButton");
    assert.deepEqual(await documentState(page), transformed);
    assert.equal((await state(page)).historyLength, invalidDepth);
    check("Nonfinite and singular native affine drafts retain validation without touching geometry/history; Escape restores the confirmed matrix and permits closing");

    await clickNative(page, "AffineButton", "InspectorViewport");
    await clickNative(page, "AffineSkewAngle", "ModalViewport");
    await page.keyboard.press("Control+a");
    await page.keyboard.insertText("20");
    await clickNative(page, "AffineSkewX", "ModalViewport");
    const skewed = await documentState(page), shear = Math.tan(20 * Math.PI / 180);
    near(skewed.nodes[3].affine_transform.c, 0.5 + shear, "Composed shear");
    near(skewed.nodes[3].x + skewed.nodes[3].affine_transform.c * 30,
        transformed.nodes[3].x + transformed.nodes[3].affine_transform.c * 30, "Skew retains local center");
    assert.deepEqual(skewed.nodes[4], transformed.nodes[4]);
    await clickNative(page, "CloseModalButton");
    await history(transformed, skewed, invalidDepth, "Centered native skew");
    check("Native horizontal skew composes with an existing affine matrix around the layer center while preserving child geometry and one-step history");

    await importFixture(page, isolatedDocument(reference, [
        fixtureNode(reference, 2, { kind: "frame", name: "Constraint frame", x: 40, y: 40,
            width: 200, height: 100, children: [3], fills: [], corner_radius: 0 }),
        fixtureNode(reference, 3, { parent: 2, name: "Pinned child", x: 20, y: 10,
            width: 40, height: 30, corner_radius: 0,
            constraints: { horizontal: "start", vertical: "start" } }),
    ], "Native constraint diagram"));
    await clickWorld(page, 80, 65);
    await clickNative(page, "ConstraintRight", "InspectorViewport");
    await clickNative(page, "ConstraintCenterY", "InspectorViewport");
    assert.deepEqual((await documentState(page)).nodes[3].constraints, { horizontal: "stretch", vertical: "center" });
    await clickWorld(page, 80, 65);
    await selectParent(2, "Constraint parent selection");
    await edit("WidthField", "300", "InspectorViewport");
    const widened = await documentState(page);
    assert.deepEqual([widened.nodes[2].width, widened.nodes[2].height], [300, 100],
        "The native width field must resize the selected constraint frame");
    assert.deepEqual([widened.nodes[3].x, widened.nodes[3].y, widened.nodes[3].width, widened.nodes[3].height], [20, 10, 140, 30]);
    await edit("HeightField", "160", "InspectorViewport");
    const resized = await documentState(page);
    assert.deepEqual([resized.nodes[2].width, resized.nodes[2].height], [300, 160],
        "The native height field must resize the selected constraint frame");
    assert.deepEqual((await state(page)).selection, [2]);
    const constrained = resized.nodes[3];
    assert.deepEqual([constrained.x, constrained.y, constrained.width, constrained.height], [20, 40, 140, 30]);
    check("Native graphical edge and center constraints drive actual parent resizing: opposing horizontal pins stretch the child and vertical centering preserves its size");

    await clickWorld(page, 80, 95);
    const selectedScene = (await state(page)).sceneFrames;
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).selectionCount === 0);
    // Escape replaces the selected-layer inspector with the page inspector.
    // Published native layout bounds alone can precede the compositor's new
    // hit-test scene; clicking those bounds early can hit the old inspector.
    await waitForNativeScene(page, selectedScene, "Page inspector after Escape");
    const beforePreset = await documentState(page), presetDepth = (await state(page)).historyLength;
    await clickNative(page, "PageFramePresetsButton", "InspectorViewport");
    await clickNative(page, "FramePreset1", "ModalViewport");
    const presetDocument = await documentState(page), presetState = await state(page);
    assert.equal(presetState.selectionCount, 1);
    const id = presetState.selection[0], created = presetDocument.nodes[id];
    assert.equal(created.kind, "frame");
    assert.equal(created.name, "Phone");
    assert.deepEqual([created.width, created.height, created.clip_content], [390, 844, true]);
    assert.equal(created.parent, presetDocument.pages[0].id);
    assert.equal(Object.keys(presetDocument.nodes).length, Object.keys(beforePreset.nodes).length + 1);
    await history(beforePreset, presetDocument, presetDepth, "Native frame preset");
    check("The native frame preset picker creates a selected, clipped and editable frame on the active page, fits it in view, and supports complete undo/redo");

    await scaleAndSliceRegressions(page, reference, check, helpers);
}

async function scaleAndSliceRegressions(page, reference, check, helpers) {
    const { fixtureNode, isolatedDocument, importFixture, documentState, state, clickNative,
        clickWorld, moveWorld, waitForNativeFrame, waitForNativeScene } = helpers;
    const artwork = fixtureNode(reference, 2, {
        kind: "rectangle", name: "Artwork", x: 20, y: 20, width: 120, height: 80,
        corner_radius: 0, corner_radii: null,
        fills: [{ visible: true, opacity: 1, paint: { type: "solid", r: 30, g: 110, b: 210, a: 255 } }],
    });
    await importFixture(page, isolatedDocument(reference, [artwork], "Scale and slice"));
    await clickNative(page, "SelectTool");
    await page.keyboard.press("v");
    await clickWorld(page, 80, 60);
    const before = await documentState(page), selected = await state(page);
    assert.deepEqual(selected.selection, [2]);

    await page.keyboard.press("k");
    await page.waitForFunction(() => {
        const view = JSON.parse(globalThis.ferroDesign.ferroDesignState());
        return view.tool === "Scale" && view.chrome.ScaleFactorField?.visible;
    });
    await waitForNativeScene(page, selected.sceneFrames, "Scale inspector");
    await clickNative(page, "ScaleFactorField", "InspectorViewport");
    await page.keyboard.press("Control+a");
    await page.keyboard.insertText("150%");
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => {
        const node = JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[2];
        return node.width === 180 && node.height === 120;
    });
    const scaled = (await documentState(page)).nodes[2];
    assert.deepEqual([scaled.x, scaled.y, scaled.width, scaled.height], [-10, 0, 180, 120],
        "150% scales the actual layer around the default center anchor");
    assert.equal((await state(page)).historyLength, selected.historyLength + 1);
    await page.keyboard.press("Control+z");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[2].width === 120);
    assert.deepEqual(await documentState(page), before);
    check("K opens the native Scale inspector; entering 150% preserves the center and one undo restores the document");

    await page.keyboard.press("Shift+0");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).zoom === 1);
    await page.keyboard.press("s");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).tool === "Slice");
    const armed = await state(page);
    await moveWorld(page, 10, 10);
    await page.mouse.down();
    try { await moveWorld(page, 70, 50); } finally { await page.mouse.up(); }
    await page.waitForFunction(() => {
        const view = JSON.parse(globalThis.ferroDesign.ferroDesignState());
        const doc = JSON.parse(globalThis.ferroDesign.ferroDesignExportJson());
        return view.selection.length === 1 && doc.nodes[view.selection[0]]?.kind === "slice";
    });
    await waitForNativeFrame(page, armed.renderedFrames, "Slice selection");
    await waitForNativeScene(page, armed.sceneFrames, "Slice export inspector");
    const sliced = await documentState(page);
    const slice = sliced.nodes[(await state(page)).selection[0]];
    assert.deepEqual([slice.width, slice.height], [60, 40]);
    const [download] = await Promise.all([
        page.waitForEvent("download", { timeout: 60000 }),
        clickNative(page, "ExportPngButton", "InspectorViewport"),
    ]);
    assert.equal(download.suggestedFilename(), slice.name + ".png");
    const stream = await download.createReadStream();
    assert.ok(stream, "A successful browser download must provide PNG bytes");
    const chunks = [];
    for await (const chunk of stream) chunks.push(chunk);
    const png = PNG.sync.read(Buffer.concat(chunks));
    assert.deepEqual([png.width, png.height], [60, 40]);
    assert.equal(png.data[(2 * png.width + 2) * 4 + 3], 0, "Padding outside the artwork is transparent");
    const inside = (20 * png.width + 20) * 4;
    assert.deepEqual([...png.data.subarray(inside, inside + 4)], [30, 110, 210, 255]);
    assert.deepEqual(await documentState(page), sliced);
    check("S drags an export slice across artwork; the native export button downloads the named 60×40 PNG with transparent padding and clipped artwork");
    await importFixture(page, isolatedDocument(reference, [
        fixtureNode(reference, 2, { kind: "rectangle", name: "Edge resize", x: 20, y: 30,
            width: 100, height: 64.5, corner_radius: 0 }),
    ], "Move tool resize"));
    await clickNative(page, "SelectTool");
    await page.keyboard.press("v");
    await clickWorld(page, 65, 62);
    assert.deepEqual((await state(page)).selection, [2]);
    const edgeBefore = await documentState(page), edgeDepth = (await state(page)).historyLength;
    // Grab the right stroke away from its midpoint, then move diagonally.
    await moveWorld(page, 120, 54);
    await page.mouse.down();
    await moveWorld(page, 145, 70);
    await page.mouse.up();
    await frames(page);
    const edgeAfter = await documentState(page);
    near(edgeAfter.nodes[2].width, 125, "Dragged edge width");
    assert.deepEqual([edgeAfter.nodes[2].x, edgeAfter.nodes[2].y, edgeAfter.nodes[2].height],
        [20, 30, 64.5], "Side resize must preserve the fixed edge and fractional inactive axis");
    await history(edgeBefore, edgeAfter, edgeDepth, "Move-tool side resize");
    check("Dragging anywhere along a selected side resizes one axis, preserves its opposite edge and fractional height, and undoes in one step");

}
