import assert from "node:assert/strict";
import { PNG } from "pngjs";

export async function rotationRegressions(page, reference, check, helpers, mode) {
    const { fixtureNode, isolatedDocument, importFixture, clickNative, moveWorld, documentState, state } = helpers;
    const make = (id, x) => fixtureNode(reference, id, {
        kind: "rectangle", name: "Rotor " + id, x, y: 40, width: 40, height: 30, corner_radius: 0,
    });
    await importFixture(page, isolatedDocument(reference, [make(2, 40), make(3, 130)], "Rotation origin"));
    await clickNative(page, "SelectTool");
    await page.keyboard.press("v");
    // The previous scenario may leave a different camera. Fit the two roots
    // through real canvas input before deriving any pointer coordinates.
    await page.keyboard.press("Control+a");
    await page.waitForFunction(() => JSON.stringify(JSON.parse(globalThis.ferroDesign.ferroDesignState()).selection) === "[2,3]");
    await fitSelection(page, helpers);
    const before = await documentState(page);
    const view = await state(page), depth = view.historyLength;
    await page.keyboard.press("Alt+r");
    assert.equal((await state(page)).tool, "Select");
    await moveWorld(page, 105, 55);
    await page.mouse.down();
    await moveWorld(page, 40, 40);
    await page.mouse.up();
    assert.deepEqual(await documentState(page), before, "Moving the origin changes only editing state");
    const angle = async target => page.waitForFunction(deg => {
        const { nodes } = JSON.parse(globalThis.ferroDesign.ferroDesignExportJson());
        return [2,3].every(id => Math.abs(nodes[id].rotation - deg) < 1e-7);
    }, target);
    const undo = async () => {
        await page.keyboard.press("Control+z");
        await angle(0);
        assert.deepEqual(await documentState(page), before);
    };
    await page.keyboard.press("Control+Alt+Shift+ArrowDown");
    await angle(15);
    const turned = (await documentState(page)).nodes;
    const rad = Math.PI / 12;
    for (const [id,dx] of [[2,20],[3,110]]) {
        assert.ok(Math.abs(turned[id].x + 20 - (40 + dx * Math.cos(rad) - 15 * Math.sin(rad))) < 1e-7);
        assert.ok(Math.abs(turned[id].y + 15 - (40 + dx * Math.sin(rad) + 15 * Math.cos(rad))) < 1e-7);
    }
    assert.equal((await state(page)).historyLength, depth + 1);
    await undo();

    const gripY = 40 - 24 / (await state(page)).zoom;
    await moveWorld(page, 105, gripY);
    await page.keyboard.down("Shift");
    try {
        await page.mouse.down();
        await moveWorld(page, 40 + 40 - gripY, 105);
        await angle(90);
        await page.mouse.up();
    } finally { await page.keyboard.up("Shift"); }
    await page.waitForFunction(previous => JSON.parse(globalThis.ferroDesign.ferroDesignState()).historyLength === previous + 1, depth);
    await undo();
    await moveWorld(page, 105, gripY);
    await page.mouse.down();
    await moveWorld(page, 40 + 40 - gripY, 105);
    await page.keyboard.press("Escape");
    await page.mouse.up();
    await angle(0);
    assert.deepEqual(await documentState(page), before);
    assert.equal((await state(page)).historyLength, depth);
    assert.equal((await state(page)).canRedo, true);
    await page.keyboard.press("Alt+r");
    check("A shared custom origin rotates two layers rigidly by keyboard and by Shift-drag; moving the origin edits no artwork, and Escape and Undo restore the originals");
    await captureOrbitRotation(page, reference, check, helpers, mode);
}

async function fitSelection(page, { state, waitForNativeFrame }) {
    const old = await state(page);
    await page.keyboard.press("Shift+2");
    await page.waitForFunction(() => {
        const v = JSON.parse(globalThis.ferroDesign.ferroDesignState());
        return v.chrome.CanvasHost.arrangeValid && v.canvas.width === v.chrome.CanvasHost.width;
    });
    await waitForNativeFrame(page, old.renderedFrames, "Fit rotation selection before deriving world gestures");
}

async function captureOrbitRotation(page, reference, check, helpers, mode) {
    const { isolatedDocument, importFixture, clickNative, moveWorld, documentState, state, captureNativeScreenshot } = helpers;
    const card = Object.values(reference.nodes).find(n => n.name === "Balance card" && n.kind === "frame");
    assert.ok(card, "The seeded Orbit balance card must be available");
    // Preserve the actual banking card's fills, typography and artwork. Isolate
    // its subtree on one page so an external rotation is visible all the way around it.
    const tree = [], pending = [card.id];
    while (pending.length) {
        const node = structuredClone(reference.nodes[pending.pop()]);
        pending.push(...node.children);
        if (node.id === card.id) { node.parent = 1; node.x = 0; node.y = 0; }
        tree.push(node);
    }
    await importFixture(page, isolatedDocument(reference, tree, "Orbit — Balance card"));
    await clickNative(page, "SelectTool");
    await page.keyboard.press("v");
    await page.keyboard.press("Control+a");
    await page.waitForFunction(id => {
        const v = JSON.parse(globalThis.ferroDesign.ferroDesignState());
        return v.selection.length === 1 && v.selection[0] === id;
    }, card.id);
    await fitSelection(page, helpers);
    const initial = await documentState(page);
    const view = await state(page), zoom = view.zoom, pivot = { x: 0, y: card.height / 2 };
    await page.keyboard.press("Alt+r");
    await moveWorld(page, card.width / 2, card.height / 2);
    await page.mouse.down();
    await moveWorld(page, pivot.x, pivot.y);
    await page.mouse.up();
    assert.deepEqual(await documentState(page), initial);
    // Hit the exterior corner rather than the top grip. Switching Shift while
    // stationary must update the angle and native inspector immediately.
    const grip = { x: card.width + 9 / zoom, y: -9 / zoom };
    const ray = { x: grip.x - pivot.x, y: grip.y - pivot.y };
    const rad = -19 * Math.PI / 180;
    await moveWorld(page, grip.x, grip.y);
    await page.mouse.down();
    try {
        await moveWorld(page, pivot.x + ray.x * Math.cos(rad) - ray.y * Math.sin(rad),
            pivot.y + ray.x * Math.sin(rad) + ray.y * Math.cos(rad));
        await page.waitForFunction(id => {
            const node = JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[id];
            return Math.abs(node.rotation + 19) < 1;
        }, card.id);
        await page.keyboard.down("Shift");
        try {
            await page.waitForFunction(id => {
                const node = JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[id];
                const v = JSON.parse(globalThis.ferroDesign.ferroDesignState());
                return Math.abs(node.rotation + 15) < 1e-7 && Number(v.chrome.RotationField.text) === -15;
            }, card.id);
            const v = await state(page), node = (await documentState(page)).nodes[card.id];
            assert.equal(v.historyLength, view.historyLength, "Preview has no committed history");
            assert.deepEqual([node.width, node.height, node.children], [card.width, card.height, card.children]);
            const angle = node.rotation * Math.PI / 180;
            assert.ok(Math.hypot(node.x + node.width / 2 - Math.cos(angle) * node.width / 2 - pivot.x,
                node.y + node.height / 2 - Math.sin(angle) * node.width / 2 - pivot.y) < 1e-6);
            const shot = await captureNativeScreenshot(mode + "-orbit-rotation-origin", {
                controls: ["CanvasHost", "Toolbar", "RotationField"],
            });
            const png = PNG.sync.read(shot);
            const project = (x, y) => ({x: Math.round(v.chrome.CanvasHost.x + v.pan.x + v.zoom * x),
                y: Math.round(v.chrome.CanvasHost.y + v.pan.y + v.zoom * y)});
            const cross = project(pivot.x, pivot.y);
            const box = v.selectionBounds, bottom = project(box.x + box.width / 2, box.y + box.height);
            const target = ink(png, cross.x - 10, cross.y - 10, cross.x + 11, cross.y + 11);
            assert.ok(target.width >= 14 && target.height >= 14, "The native origin crosshair is painted at the fixed left edge");
            const badge = ink(png, bottom.x - 60, bottom.y - 48, bottom.x + 61, bottom.y + 44);
            assert.ok(badge.width >= 32 && badge.height >= 15, "The live angle badge is visible beside the transformed selection");
        } finally { await page.keyboard.up("Shift"); }
        await page.waitForFunction(id => Math.abs(JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[id].rotation + 19) < 1, card.id);
        await page.keyboard.press("Escape");
    } finally { await page.mouse.up(); }
    await page.waitForFunction(id => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).nodes[id].rotation === 0, card.id);
    assert.deepEqual(await documentState(page), initial);
    await page.keyboard.press("Alt+r");
    check("Native Orbit artwork rotates from its left edge by an outside-corner drag; stationary Shift snaps from −19° to −15°, the inspector and pixels agree, and Escape restores the card");
}

function ink(png, left, top, right, bottom) {
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (let y = Math.max(0, top); y < Math.min(png.height, bottom); y++) {
        for (let x = Math.max(0, left); x < Math.min(png.width, right); x++) {
            const i = 4 * (y * png.width + x), d = png.data;
            if (Math.abs(d[i] - 9) < 5 && Math.abs(d[i + 1] - 109) < 7 && Math.abs(d[i + 2] - 174) < 8) {
                minX = Math.min(minX, x); maxX = Math.max(maxX, x);
                minY = Math.min(minY, y); maxY = Math.max(maxY, y);
            }
        }
    }
    return {width: maxX - minX + 1, height: maxY - minY + 1};
}
