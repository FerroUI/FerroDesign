import assert from "node:assert/strict";

export async function nudgePreferences(page, reference, check, helpers) {
    const { fixtureNode, isolatedDocument, importFixture, documentState, state, clickWorld, clickNative, frames } = helpers;
    await importFixture(page, isolatedDocument(reference, [
        fixtureNode(reference, 2, { x: 48.375, y: 32, width: 80, height: 56 }),
    ], "Nudge preferences"));
    await clickWorld(page, 82, 56);
    const original = await documentState(page);
    const open = async () => {
        assert.equal(await page.evaluate(() => globalThis.ferroDesign.ferroDesignCommand("nudge-settings")), true);
        await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.NudgeSmall?.focused);
    };
    const replace = async (name, value) => {
        await clickNative(page, name);
        await page.keyboard.press("Control+a");
        await page.keyboard.insertText(value);
    };
    await open();
    const initial = (await state(page)).chrome;
    assert.equal(initial.NudgeSmall.text, "1");
    assert.equal(initial.NudgeBig.text, "10");
    await replace("NudgeSmall", "0.125");
    await replace("NudgeBig", "NaN");
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.NudgeError?.visible);
    await replace("NudgeBig", "8.5");
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.CanvasHost.focusWithin);
    const stored = () => page.evaluate(() => JSON.parse(localStorage.getItem("ferrodesign.nudgeAmounts.v1")));
    assert.deepEqual(await stored(), [0.125, 8.5]);
    const before = await state(page);
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("Shift+ArrowLeft");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[2].x === 40);
    assert.equal((await state(page)).historyLength, before.historyLength + 2);
    await page.keyboard.press("Control+z");
    await page.keyboard.press("Control+z");
    assert.deepEqual(await documentState(page), original);
    const size = page.viewportSize();
    try {
        await page.setViewportSize({ width: 320, height: 568 });
        await frames(page);
        await open();
        const phone = (await state(page)).chrome;
        for (const name of ["NudgeSmall", "NudgeBig", "NudgeDone", "NudgeReset", "CloseModalButton"]) {
            assert.ok(phone[name].height >= 44 && phone[name].width >= 44, name);
        }
        assert.equal(phone.NudgeBig.text, "8.5");
        await replace("NudgeSmall", "4");
        await page.keyboard.press("Escape");
        await open();
        assert.equal((await state(page)).chrome.NudgeSmall.text, "0.125");
        await clickNative(page, "NudgeReset");
        await clickNative(page, "CloseModalButton");
        assert.deepEqual(await documentState(page), original);
        assert.deepEqual(await stored(), [1, 10]);
        check("Fractional and large nudges edit precise positions and undo exactly; phone preferences validate, retain applied values, and discard Escape drafts");
    } finally {
        await page.setViewportSize(size);
        await frames(page);
    }
}
