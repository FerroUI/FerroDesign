import assert from "node:assert/strict";

// Exercise the production lifecycle listener and actual FerroUI TextBoxes.
// No test-only command writes the document or marks a draft dirty for us.
export async function inspectorLifecycleRegressions(page, reference, check, helpers) {
    const { fixtureNode, isolatedDocument, importFixture, documentState, state,
        clickWorld, clickNative, frames, flushStorage, waitForReload } = helpers;
    const storage = () => page.evaluate(() => globalThis.ferroDesignStorage.status());
    const durable = () => page.evaluate(async () =>
        JSON.parse((await globalThis.ferroDesignStorage.backend.load()).text));
    const pagehide = () => page.evaluate(() => globalThis.dispatchEvent(new Event("pagehide")));
    const draft = async (name, value, container) => {
        await clickNative(page, name, container);
        assert.equal((await state(page)).chrome[name].focused, true, name + " must own native input focus");
        await page.keyboard.press("Control+a");
        await page.keyboard.insertText(value);
        await frames(page);
        assert.equal((await state(page)).chrome[name].text, value);
        assert.equal((await storage()).draftDirty, true, name + " must notify the host before its native commit");
    };
    const exactHistory = async (before, after, depth, label) => {
        assert.equal((await state(page)).historyLength, depth + 1, label + " adds exactly one transaction");
        await page.keyboard.press("Control+z");
        await frames(page);
        assert.deepEqual(await documentState(page), before, label + " undoes every document field");
        assert.equal((await state(page)).historyLength, depth);
        await page.keyboard.press("Control+Shift+z");
        await frames(page);
        assert.deepEqual(await documentState(page), after, label + " redoes every document field");
        assert.equal((await state(page)).historyLength, depth + 1);
    };

    const fixture = isolatedDocument(reference, [fixtureNode(reference, 2, {
        name: "Lifecycle rectangle", x: 40, y: 40, width: 220, height: 160,
        effects: [],
    })], "Imported customer design");
    fixture.pages[0].name = "Account settings";
    await importFixture(page, fixture);
    assert.equal((await state(page)).canvasCaption, "Imported customer design / Account settings",
        "Native canvas caption follows the imported document and active page");
    await clickWorld(page, 130, 110);
    assert.equal((await flushStorage(page)).saved, true);
    const beforeWidth = await documentState(page), widthDepth = (await state(page)).historyLength;
    await draft("WidthField", "287.5", "InspectorViewport");
    assert.deepEqual(await documentState(page), beforeWidth, "Typing a width is a native draft until commit");
    await pagehide();
    assert.equal((await flushStorage(page)).saved, true);
    const resized = await documentState(page);
    assert.equal(resized.nodes[2].width, 287.5);
    assert.deepEqual(await durable(), resized, "Pagehide must commit focused inspector geometry to IndexedDB");
    assert.equal((await storage()).draftDirty, false);
    await exactHistory(beforeWidth, resized, widthDepth, "Lifecycle width edit");
    assert.equal((await flushStorage(page)).saved, true);
    await page.reload({ waitUntil: "domcontentloaded", timeout: 120000 });
    await waitForReload(page);
    assert.deepEqual(await documentState(page), resized, "A real reload recovers the focused width draft");
    assert.equal((await state(page)).canvasCaption, "Imported customer design / Account settings");
    check("Native inspector geometry drafts commit once at pagehide, preserve exact undo/redo, survive IndexedDB reload, and display the current document/page caption");

    await clickWorld(page, 130, 110);
    await clickNative(page, "AddEffectButton", "InspectorViewport");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[2].effects.length === 1);
    assert.equal((await flushStorage(page)).saved, true);
    const beforeEffect = await documentState(page), effectDepth = (await state(page)).historyLength;
    await draft("EffectRadius0", "18.75", "ModalViewport");
    assert.deepEqual(await documentState(page), beforeEffect);
    await pagehide();
    assert.equal((await flushStorage(page)).saved, true);
    const effected = await documentState(page);
    assert.equal(effected.nodes[2].effects[0].radius, 18.75);
    assert.deepEqual(await durable(), effected, "Focused dynamic effect fields must join the same lifecycle boundary");
    await clickNative(page, "EffectDone");
    await exactHistory(beforeEffect, effected, effectDepth, "Lifecycle effect edit");
    assert.equal((await flushStorage(page)).saved, true);
    check("Native dynamic effect drafts join lifecycle saving with complete typed properties and one exact history transaction");

    await clickNative(page, "EffectButton", "InspectorViewport");
    const invalidDepth = (await state(page)).historyLength;
    await draft("EffectRadius0", "NaN", "ModalViewport");
    await pagehide();
    await frames(page);
    assert.equal((await storage()).draftDirty, true, "Rejected drafts keep the actual unload guard active");
    assert.equal((await storage()).phase, "error");
    assert.equal((await state(page)).chrome.EffectRadius0.text, "NaN", "Validation must retain the field for correction");
    assert.equal((await state(page)).chrome.EffectRadius0Error.visible, true);
    assert.deepEqual(await documentState(page), effected);
    assert.deepEqual(await durable(), effected, "A failed draft cannot replace the last durable snapshot");
    assert.equal((await state(page)).historyLength, invalidDepth);
    await clickNative(page, "EffectRadius0", "ModalViewport");
    await page.keyboard.press("Escape");
    await frames(page);
    assert.equal((await state(page)).chrome.EffectRadius0.text, "18.75");
    assert.equal((await storage()).draftDirty, false);
    assert.equal((await storage()).phase, "saved", "Discarding a draft clears only its validation error");
    await clickNative(page, "EffectDone");
    assert.deepEqual(await documentState(page), effected);
    assert.equal((await state(page)).historyLength, invalidDepth);
    check("Invalid lifecycle drafts retain native input and validation, preserve document/history/durable storage, and Escape clears their unload guard without a delayed blur edit");

    const beforeName = await documentState(page), nameDepth = (await state(page)).historyLength;
    const confirmed = "Confirmed inspector · Ω 🚀";
    await draft("NameField", confirmed, "InspectorViewport");
    await page.keyboard.press("Control+a");
    const ime = page.locator(".ferroui-input-element");
    assert.equal(await ime.count(), 1);
    await ime.evaluate((input) => {
        if (document.activeElement !== input) throw new Error("Native inspector must own the browser IME bridge");
        input.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" }));
        input.dispatchEvent(new CompositionEvent("compositionupdate", { bubbles: true, data: "未確定" }));
    });
    assert.equal((await state(page)).chrome.NameField.composing, true);
    assert.deepEqual(await documentState(page), beforeName, "Inspector preedit cannot enter the design model");
    await pagehide();
    assert.equal((await flushStorage(page)).saved, true);
    const renamed = await documentState(page);
    assert.equal(renamed.nodes[2].name, confirmed, "Lifecycle focus release restores the last confirmed input before IME detachment");
    assert.deepEqual(await durable(), renamed);
    await exactHistory(beforeName, renamed, nameDepth, "Inspector IME lifecycle edit");
    assert.equal((await flushStorage(page)).saved, true);
    await page.reload({ waitUntil: "domcontentloaded", timeout: 120000 });
    await waitForReload(page);
    assert.deepEqual(await documentState(page), renamed, "Reload restores confirmed Unicode, never a preedit deletion or unconfirmed characters");
    check("Pagehide during native inspector composition saves the confirmed Unicode draft once and survives a real reload without persisting IME preedit");

    await mixedSelectionAndDock(page, reference, check, helpers);
}

export async function mixedSelectionAndDock(page, reference, check, helpers) {
    const { fixtureNode, isolatedDocument, importFixture, documentState, state, clickWorld, clickNative } = helpers;
    const box = (id, x, width) => fixtureNode(reference, id, {
        kind: "rectangle", name: "Box " + id, x, y: 40, width, height: 60,
        fills: [{ visible: true, opacity: 1, paint: { type: "solid", r: 30, g: 110, b: 210, a: 255 } }],
    });
    await importFixture(page, isolatedDocument(reference, [box(2, 20, 64), box(3, 140, 96)], "Inspector selection"));
    await clickNative(page, "SelectTool");
    await page.keyboard.press("v");
    await clickWorld(page, 50, 70);
    await page.keyboard.down("Shift");
    try { await clickWorld(page, 175, 70); } finally { await page.keyboard.up("Shift"); }
    const selected = await state(page);
    assert.deepEqual(selected.selection, [2, 3]);
    assert.equal(selected.chrome.WidthField.text, "");
    assert.equal(selected.chrome.HeightField.text, "60");
    const before = await documentState(page);
    await clickNative(page, "WidthField", "InspectorViewport");
    await page.keyboard.insertText("64");
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[3].width === 64);
    assert.equal((await state(page)).historyLength, selected.historyLength + 1);
    await page.keyboard.press("Control+z");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[3].width === 96);
    assert.deepEqual(await documentState(page), before);
    check("Mixed widths accept the first layer's existing value as a batch edit, while a common height remains visible and undo restores both layers");

    await clickNative(page, "WidthField", "InspectorViewport");
    await page.keyboard.insertText("(Mixed / 2) + 6");
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => {
        const { nodes } = JSON.parse(globalThis.ferroDesign.ferroDesignExportJson());
        return nodes[2].width === 38 && nodes[3].width === 54;
    });
    assert.equal((await state(page)).historyLength, selected.historyLength + 1);
    await page.keyboard.press("Control+z");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[3].width === 96);
    assert.deepEqual(await documentState(page), before);
    check("A mixed width equation evaluates each original size, commits once and restores both layers on undo");

    await page.keyboard.press("F6");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.SelectTool.focused === true);
    await page.keyboard.press("End");
    assert.equal((await state(page)).chrome.ActionsButton.focused, true);
    await page.keyboard.press("Home");
    assert.equal((await state(page)).chrome.SelectTool.focused, true);
    await page.keyboard.press("Escape");
    assert.equal((await state(page)).chrome.CanvasHost.focusWithin, true);
    assert.deepEqual(await documentState(page), before);
    check("F6 enters the tool dock; Home, End and Escape navigate and restore native canvas focus without editing the document");
}
