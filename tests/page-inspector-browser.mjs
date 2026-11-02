import assert from "node:assert/strict";
import { PNG } from "pngjs";

export async function pageInspectorRegressions(page, reference, check, helpers, output, mode) {
    const { fixtureNode, isolatedDocument, importFixture, documentState, state, clickWorld, clickNative, frames, captureNativeScreenshot } = helpers;
    const fixture = isolatedDocument(reference, [fixtureNode(reference, 2, {
        x: 0, y: 0, width: 160, height: 120, corner_radius: 0,
    })], "Contextual native inspector");
    await importFixture(page, fixture);
    const authored = await documentState(page);
    let view = await state(page);
    const initialHistory = view.historyLength;
    assert.equal(view.selectionCount, 0);
    assert.equal(view.chrome.PageInspector.visible, true);
    assert.equal(view.chrome.DesignInspector.visible, false);
    await clickNative(page, "PageBackgroundField", "InspectorViewport");
    await page.keyboard.press("Control+a");
    await page.keyboard.insertText("E6E9EF");
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => {
        const color = JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).pages[0].background;
        return color.r === 230 && color.g === 233 && color.b === 239 && color.a === 255;
    });
    await frames(page);
    view = await state(page);
    const committedHistory = view.historyLength;
    assert.ok(committedHistory === initialHistory + 1 || committedHistory === initialHistory,
        "A page background edit adds one history entry, allowing bounded-history eviction");
    const bytes = await captureNativeScreenshot(mode + "-page-inspector-native", {
        controls: ["CanvasHost", "PageInspector", "PageBackgroundField"],
    });
    const png = PNG.sync.read(bytes);
    const x = Math.floor(view.chrome.CanvasHost.x + 10), y = 90;
    const actual = [...png.data.subarray((y * png.width + x) * 4, (y * png.width + x) * 4 + 4)];
    assert.ok([230, 233, 239, 255].every((channel, index) => Math.abs(channel - actual[index]) <= 3),
        "The native page field must change actual canvas background pixels: " + JSON.stringify(actual));
    assert.equal(await page.evaluate(() => globalThis.ferroDesign.ferroDesignCommand("undo")), true);
    await frames(page);
    assert.deepEqual(await documentState(page), authored, "Page background has one exact document undo");
    assert.equal((await state(page)).historyLength, committedHistory - 1);
    await clickNative(page, "PageBackgroundField", "InspectorViewport");
    await page.keyboard.press("Control+a");
    await page.keyboard.insertText("not a color");
    await page.keyboard.press("Enter");
    await frames(page);
    assert.deepEqual(await documentState(page), authored, "Invalid page colors leave the document and history unchanged");
    assert.equal((await state(page)).historyLength, committedHistory - 1);
    await clickWorld(page, 80, 60);
    await frames(page);
    view = await state(page);
    assert.equal(view.chrome.PageInspector.visible, false);
    assert.equal(view.chrome.DesignInspector.visible, true);
    const canvas = view.chrome.CanvasHost;
    await page.mouse.click(canvas.x + 10, 90);
    await frames(page);
    view = await state(page);
    assert.equal(view.selectionCount, 0);
    assert.equal(view.chrome.PageInspector.visible, true);
    assert.equal(view.chrome.DesignInspector.visible, false);
    check("Empty selection exposes native page background and local resources; native color edits change canvas pixels, validate atomically, and undo exactly");

    const setPercent = async percent => {
        await clickNative(page, "ZoomValueButton");
        await page.keyboard.type(String(percent));
        await page.keyboard.press("Enter");
        await page.waitForFunction(value => JSON.parse(globalThis.ferroDesign.ferroDesignState()).zoom === value, percent / 100);
    };
    const camera = ({ zoom, pan }) => ({ zoom, pan });
    const activateRow = async (row, id) => {
        await page.mouse.click(row.x + row.width / 2, row.y + row.height / 2);
        await page.waitForFunction(value => JSON.parse(globalThis.ferroDesign.ferroDesignState()).activePage === value, id);
        await frames(page);
    };
    await setPercent(37.5);
    const firstPage = await state(page);
    await clickNative(page, "AddPageButton");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).pageCount === 2);
    const beforePages = await documentState(page);
    const current = (await state(page)).activePage;
    const depth = (await state(page)).historyLength;
    await frames(page);
    assert.notEqual((await state(page)).zoom, firstPage.zoom, "An empty page begins with its own view");
    await setPercent(162.5);
    const secondPage = await state(page);
    await activateRow(firstPage.chrome.ActivePage, firstPage.activePage);
    assert.deepEqual(camera(await state(page)), camera(firstPage));
    await activateRow(secondPage.chrome.ActivePage, secondPage.activePage);
    assert.deepEqual(camera(await state(page)), camera(secondPage));
    assert.deepEqual(await documentState(page), beforePages);
    assert.equal((await state(page)).historyLength, depth);
    check("Page row navigation restores independent zoom and pan without changing artwork or undo history");
    await clickNative(page, "PageActionsButton");
    await clickNative(page, "PageRename");
    await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.PageRenameEditor?.focused);
    await page.keyboard.insertText("Checkout – review");
    await page.keyboard.press("Enter");
    await page.waitForFunction(id =>
        JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).pages.find(p => p.id === id)?.name === "Checkout – review", current);
    const afterRename = await documentState(page);
    assert.equal((await state(page)).historyLength, depth + 1);
    await clickNative(page, "PageActionsButton");
    await clickNative(page, "PageMoveUp");
    await page.waitForFunction(id => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).pages[0].id === id, current);
    assert.equal((await state(page)).activePage, current);
    assert.equal((await state(page)).historyLength, depth + 2);
    await page.keyboard.press("Control+z");
    await frames(page);
    assert.deepEqual(await documentState(page), afterRename);
    await page.keyboard.press("Control+z");
    await frames(page);
    assert.deepEqual(await documentState(page), beforePages);
    check("The native page menu renames in place and reorders pages while preserving the active page and exact undo");

    await activateRow(firstPage.chrome.ActivePage, firstPage.activePage);
    const parent = await state(page), original = await documentState(page);
    const menuAction = async id => {
        await clickNative(page, "PageActionsButton");
        if (id === "PageDuplicate") {
            await captureNativeScreenshot(mode + "-page-actions", { controls: ["PageDuplicate", "PageDelete", "PageRename"] });
        }
        await clickNative(page, id);
        await frames(page);
    };
    await menuAction("PageDuplicate");
    const duplicated = await documentState(page), duplicate = duplicated.pages[1], src = original.pages[0];
    assert.equal(duplicate.name, "Copy of " + src.name);
    assert.equal(duplicated.pages[0].id, src.id);
    assert.equal(duplicated.pages[2].id, current);
    assert.equal(duplicate.children.length, src.children.length);
    assert.ok(src.children.length > 0 && duplicate.children.every(id => !original.nodes[id]));
    assert.equal((await state(page)).activePage, duplicate.id);
    assert.deepEqual(camera(await state(page)), camera(parent));
    assert.equal((await state(page)).historyLength, parent.historyLength + 1);
    await setPercent(75);
    const copyView = camera(await state(page));
    await menuAction("PageDelete");
    assert.equal((await state(page)).activePage, current);
    assert.deepEqual((await documentState(page)).nodes, original.nodes);
    await page.keyboard.press("Control+z");
    await frames(page);
    assert.equal((await state(page)).activePage, duplicate.id);
    assert.deepEqual(camera(await state(page)), copyView);
    assert.deepEqual(await documentState(page), duplicated);
    await page.keyboard.press("Control+z");
    await frames(page);
    assert.equal((await state(page)).activePage, src.id);
    assert.deepEqual(await documentState(page), original);
    check("Duplicating a populated page preserves its view and gives layers fresh IDs; deleting and undoing restores the exact page and camera");

    const row = secondPage.chrome.ActivePage, first = firstPage.chrome.ActivePage;
    const beginReorder = async () => {
        await page.mouse.move(row.x + row.width / 2, row.y + row.height / 2);
        await page.mouse.down();
        await page.mouse.move(first.x + first.width / 2, first.y + 3, { steps: 5 });
    };
    await beginReorder();
    // Hold past the native 400ms tooltip delay. A popup from this hovered page
    // must not intercept the next press on the neighboring row after Escape.
    await page.waitForTimeout(500);
    await frames(page);
    const preview = PNG.sync.read(await captureNativeScreenshot(mode + "-page-reorder", { controls: ["PagesViewport", "ActivePage"] }));
    let blue = 0;
    for (let y = Math.floor(first.y); y < first.y + 3; y++) {
        for (let x = Math.ceil(first.x + 4); x < first.x + first.width - 4; x++) {
            const i = (y * preview.width + x) * 4;
            if (preview.data[i] < 90 && preview.data[i+1] > 90 && preview.data[i+2] > preview.data[i+1] + 40) blue++;
        }
    }
    assert.ok(blue >= first.width * 0.6, "The page insertion line is visible at its target edge");
    assert.deepEqual(await documentState(page), original);
    await page.keyboard.press("Escape");
    await page.mouse.up();
    assert.deepEqual(await documentState(page), original);
    await beginReorder();
    await page.mouse.up();
    await page.waitForFunction(id => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).pages[0].id === id, current);
    assert.equal((await state(page)).activePage, src.id);
    assert.deepEqual(camera(await state(page)), camera(parent));
    await page.keyboard.press("Control+z");
    await frames(page);
    assert.deepEqual(await documentState(page), original);
    check("Dragging an inactive page reorders on release, keeps the current canvas and camera, and cancels or undoes exactly");

    const desktop = page.viewportSize(), phoneDocument = structuredClone(original);
    for (let i = 0; i < 10; i++) {
        phoneDocument.pages.push({ ...phoneDocument.pages[1], id: phoneDocument.next_id++, name: "Mobile scene " + i, children: [] });
    }
    const input = await page.context().newCDPSession(page);
    try {
        await importFixture(page, phoneDocument);
        await page.setViewportSize({width: 390, height: 844});
        await page.waitForFunction(() => JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.CompactLayers.visible);
        await clickNative(page, "CompactLayers");
        await clickNative(page, "PageActionsButton");
        const phone = (await state(page)).chrome;
        for (const name of ["PageActionsButton", "AddPageButton", "ActivePage", "PageRename", "PageDuplicate", "PageMoveUp", "PageMoveDown", "PageDelete"]) {
            const c = phone[name];
            assert.ok(c.width >= 44 && c.height >= 44 && c.x >= 0 && c.x + c.width <= 391 && c.y >= 0 && c.y + c.height <= 845,
                name + " has an on-screen phone touch target");
        }
        await captureNativeScreenshot(mode + "-phone-page-actions", {controls: ["PageDuplicate", "PageDelete", "ActivePage"]});
        await page.keyboard.press("Escape");
        await page.waitForFunction(() => !JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.PageDuplicate);
        await input.send("Emulation.setTouchEmulationEnabled", {enabled: true});
        const touch = (type, point) => input.send("Input.dispatchTouchEvent", {
            type, touchPoints: point ? [{...point, id: 1, radiusX: 2, radiusY: 2, force: 1}] : [],
        });
        const origin = phone.ActivePage, firstId = phoneDocument.pages[0].id, secondId = phoneDocument.pages[1].id;
        const second = {x: origin.x + origin.width / 2, y: origin.y + 1.5 * origin.height + 2};
        const top = {x: second.x, y: origin.y + 3};
        const held = async () => {
            await touch("touchStart", second);
            // Allow the platform to recognize a stationary touch hold before choosing a move or release.
            await page.waitForTimeout(600);
            const during = await state(page);
            assert.equal(during.activePage, firstId);
            assert.ok(!during.chrome.PageDuplicate, "The context menu waits until a stationary finger lifts");
        };
        await held();
        await touch("touchMove", {x: second.x, y: second.y - 12});
        await touch("touchMove", top);
        await frames(page);
        const beforeDrop = await documentState(page);
        assert.deepEqual(beforeDrop.pages, phoneDocument.pages);
        await captureNativeScreenshot(mode + "-phone-page-reorder", {controls: ["PagesViewport", "ActivePage"]});
        await touch("touchEnd");
        await page.waitForFunction(id => JSON.parse(globalThis.ferroDesign.ferroDesignExportJson()).pages[0].id === id, secondId);
        assert.equal((await state(page)).activePage, firstId);
        await page.keyboard.press("Control+z");
        await frames(page);
        assert.deepEqual(await documentState(page), beforeDrop);
        await held();
        await touch("touchEnd");
        await page.waitForFunction(id => {
            const v = JSON.parse(globalThis.ferroDesign.ferroDesignState());
            return v.activePage === id && v.chrome.PageDuplicate?.visible;
        }, secondId);
        await page.keyboard.press("Escape");
        await page.waitForFunction(() => !JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.PageDuplicate);
        const from = {x: second.x, y: origin.y + 2.5 * origin.height + 4};
        await touch("touchStart", from);
        for (const step of [12, 30, 52, 72]) await touch("touchMove", {x: from.x, y: from.y - step});
        await touch("touchEnd");
        await page.waitForFunction(y => JSON.parse(globalThis.ferroDesign.ferroDesignState()).chrome.ActivePage.y < y - 5,
            origin.y + origin.height + 2);
        assert.deepEqual(await documentState(page), beforeDrop);
        check("On a phone, held touch can drag or open page actions on release, a quick swipe scrolls, and page controls have 44-DIP targets");
    } finally {
        await input.send("Emulation.setTouchEmulationEnabled", {enabled: false});
        await input.detach();
        await page.setViewportSize(desktop);
        await frames(page);
    }

}
