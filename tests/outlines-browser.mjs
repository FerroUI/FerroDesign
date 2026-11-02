import assert from "node:assert/strict";
import { PNG } from "pngjs";

export async function outlineView(page, reference, check, helpers, mode) {
    const { fixtureNode, isolatedDocument, importFixture, state, documentState, clickNative, clickWorld,
        frames, waitForNativeFrame, captureNativeScreenshot } = helpers;
    const make = (id, spec) => fixtureNode(reference, id, { corner_radius: 0, ...spec });
    const card = make(2, { kind: "frame", x: 50, y: 40, width: 100, height: 90, children: [3], clip_content: true });
    const overflow = make(3, { kind: "ellipse", parent: 2, x: 70, y: 20, width: 80, height: 60 });
    const hidden = make(4, { kind: "rectangle", x: 240, y: 50, width: 80, height: 60, visible: false });
    const far = make(5, { kind: "rectangle", x: 330, y: 135, width: 24, height: 28 });
    await importFixture(page, isolatedDocument(reference, [card, overflow, hidden, far], "Inspect layer outlines"));
    await clickNative(page, "SelectTool");
    const original = await documentState(page), previous = await state(page);
    await page.keyboard.press("Shift+1");
    await waitForNativeFrame(page, previous.renderedFrames, "Fit outline comparison");
    const initial = await state(page);
    const command = name => page.evaluate(name => globalThis.ferroDesign.ferroDesignCommand(name), name);
    const expected = values => page.waitForFunction(values => {
        const view = JSON.parse(globalThis.ferroDesign.ferroDesignState()).outlineView;
        return Object.entries(values).every(([k,v]) => view[k] === v);
    }, values);
    if (!initial.outlineView.enabled) await page.keyboard.press("Control+Shift+o");
    await expected({ enabled: true });
    if (initial.outlineView.objectBounds) await command("outline-bounds");
    if (initial.outlineView.includeHidden) await command("outline-hidden");
    await expected({ objectBounds: false, includeHidden: false });
    await page.mouse.move(5, 5);
    await frames(page);
    const view = await state(page);
    const png = PNG.sync.read(await captureNativeScreenshot(mode + "-layer-outlines", {controls:["CanvasHost","Toolbar"]}));
    const sample = (x,y) => {
        const sx = Math.round(view.chrome.CanvasHost.x + view.pan.x + view.zoom*x);
        const sy = Math.round(view.chrome.CanvasHost.y + view.pan.y + view.zoom*y);
        assert.ok(sx >= 2 && sy >= 2 && sx + 2 < png.width && sy + 2 < png.height);
        let dark = 0;
        for (let row=sy-2; row<=sy+2; row++) for (let col=sx-2; col<=sx+2; col++) {
            const i=4*(row*png.width+col);
            if (Math.max(...png.data.subarray(i,i+3)) < 218) dark++;
        }
        return dark;
    };
    assert.ok(sample(200,90) > 0, "Clipped ellipse edge is drawn in outline view");
    assert.equal(sample(80,85), 0, "Authored card fill is suppressed");
    assert.equal(sample(280,50), 0, "Hidden geometry starts excluded");
    await clickWorld(page,200,90);
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).selection.join() === "3");
    await command("outline-hidden");
    await clickWorld(page,280,50);
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).selection.join() === "4");
    assert.deepEqual(await documentState(page), original);
    assert.equal((await state(page)).historyLength, initial.historyLength);
    await command("outline-hidden");
    if (initial.outlineView.objectBounds) await command("outline-bounds");
    if (initial.outlineView.includeHidden) await command("outline-hidden");
    if (!initial.outlineView.enabled) await page.keyboard.press("Control+Shift+o");
    await expected(initial.outlineView);
    check("Outline view reveals clipped edges, optionally selects hidden artwork, and restores the normal view without document or history changes");
}
