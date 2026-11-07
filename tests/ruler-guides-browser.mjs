import assert from "node:assert/strict";

export async function rulerGuides(page, reference, check, helpers) {
    const { isolatedDocument, importFixture, state, documentState, clickNative, frames } = helpers;
    await importFixture(page, isolatedDocument(reference, [], "Ruler guides"));
    await clickNative(page, "SelectTool");
    const initial = await state(page);
    if (!initial.rulersVisible) await page.keyboard.press("Shift+r");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).rulersVisible);
    await frames(page);
    const view = await state(page), c = view.chrome.CanvasHost;
    const point = (x, y) => [Math.round(c.x+x),Math.round(c.y+y)];
    const prefix = point(7, c.height * 0.4);
    const finish = point(c.width * 0.36,c.height * 0.4);
    const pageId = view.activePage;
    const guides = async () => (await documentState(page)).guides[pageId] ?? [];
    await page.mouse.move(...prefix); await page.mouse.down();
    await page.mouse.move(...finish, {steps: 4});
    assert.deepEqual(await guides(), []);
    await page.mouse.up();
    const count = n => page.waitForFunction(({id,n}) =>
        (JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).guides[id] ?? []).length === n, {id:pageId,n});
    await count(1);
    const [vertical] = await guides();
    assert.equal(vertical.horizontal,false);
    assert.ok(Math.abs(vertical.position - (finish[0]-c.x-view.pan.x)/view.zoom) <= 1);
    assert.equal((await state(page)).historyLength,initial.historyLength+1);
    const guideX = Math.round(c.x + view.pan.x + vertical.position * view.zoom);
    await page.keyboard.down("Alt");
    try {
        await page.mouse.move(guideX,finish[1]); await page.mouse.down();
        await page.mouse.move(guideX+60,finish[1],{steps:3}); await page.mouse.up();
    } finally { await page.keyboard.up("Alt"); }
    await count(2);
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.RulerPosition1?.focused);
    await page.keyboard.press("Control+a");
    await page.keyboard.insertText("-12.75");
    await page.keyboard.press("Enter");
    await page.waitForFunction(id => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).guides[id][1].position === -12.75,pageId);
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => !JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.ModalBackdrop.visible);
    const copy = await guides();
    await page.keyboard.press("Delete"); await count(1);
    assert.deepEqual(await guides(),[vertical]);
    await page.keyboard.press("Control+z"); await count(2);
    assert.deepEqual(await guides(),copy);
    await page.keyboard.press("Shift+r");
    check("Rulers create a page guide from real pointer input, Option-drag duplicates it, and Delete and Undo restore exact positions");
    const desktop = page.viewportSize();
    try {
        await page.setViewportSize({ width: 320, height: 568 });
        await frames(page);
        await page.evaluate(() => globalThis.ferroDesign.ferroDesignCommand("ruler-guide-settings"));
        await page.waitForFunction(() => Object.entries(JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome)
            .some(([id, c]) => id.startsWith("RulerPosition") && c.focused && c.height >= 44));
        const { chrome } = await state(page);
        const focused = Object.keys(chrome).find(id => id.startsWith("RulerPosition") && chrome[id].focused);
        for (const name of ["RulerAddVertical", "RulerAddHorizontal", "RulerPosition0",
            "RulerApply0", "RulerRemove0", "CloseModalButton", "RulerDone"]) {
            assert.ok(chrome[name].width >= 44 && chrome[name].height >= 44, name + " has a phone-sized target");
        }
        await page.keyboard.press("Control+a");
        await page.keyboard.insertText("not a position");
        await page.keyboard.press("Enter");
        const error = focused.replace("Position", "Error");
        await page.waitForFunction(({focused,error}) => {
            const c = JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome;
            return c[focused]?.focused && c[error]?.visible;
        }, {focused,error});
        assert.deepEqual(await guides(), copy);
        await page.keyboard.press("Escape");
        await page.waitForFunction(() => !JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.ModalBackdrop.visible);
        check("At 320 px ruler controls have 44-DIP targets, open on an editable position, and validate inline without changing guides");
    } finally {
        await page.setViewportSize(desktop);
        await frames(page);
    }

}
