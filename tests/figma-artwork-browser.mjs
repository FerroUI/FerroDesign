import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { PNG } from "pngjs";

const hashes = {
    svg: "6708eae2410ff57306ace3ccb57d5051724fb450dc1d874f93811cc6d41cbbf1",
    png: "587628710821fa4cb8fb7f563bdf923446765858e8398c50ede8b50c8b163620",
};
const sha256 = bytes => createHash("sha256").update(bytes).digest("hex");

export async function figmaArtworkReference(page, reference, check, helpers, output, mode) {
    const directory = process.env.FERRODESIGN_FIGMA_REFERENCE;
    assert.ok(directory, "Set FERRODESIGN_FIGMA_REFERENCE to the downloaded gid1-11 pair (docs/FIGMA_ARTWORK_REFERENCE.md)");
    const load = async extension => {
        const bytes = await readFile(resolve(directory, "gid1-11." + extension));
        assert.equal(sha256(bytes), hashes[extension], "The source " + extension + " must match the pinned CANVAS revision");
        return bytes;
    };
    const [source, expectedBytes] = await Promise.all([load("svg"), load("png")]);
    const expected = PNG.sync.read(expectedBytes);
    assert.deepEqual([expected.width, expected.height], [750, 1624]);
    // FerroDesign exports one pixel per document unit. Change only the outer
    // viewport dimensions, keeping the original viewBox and all inner artwork.
    const svg = source.toString("utf8");
    const resized = svg.replace('<svg width="375" height="812"', '<svg width="750" height="1624"');
    assert.notEqual(resized, svg);
    assert.doesNotMatch(svg, /<image\b|data:image\//i);
    await helpers.importFixture(page, helpers.isolatedDocument(reference, [], "Figma artwork comparison"));
    await helpers.clickNative(page, "SelectTool");
    const [chooser] = await Promise.all([page.waitForEvent("filechooser"), page.keyboard.press("Control+o")]);
    await chooser.setFiles({ name: "gid1-11.svg", mimeType: "image/svg+xml", buffer: Buffer.from(resized) });
    await page.waitForFunction(() => {
        const s = JSON.parse(globalThis.ferroDesign.ferroDesignState());
        return s.selectionCount === 1 && s.selectionBounds?.width === 750 && s.selectionBounds?.height === 1624;
    });
    const view = await helpers.state(page);
    assert.deepEqual(view.selectionBounds, { x: 0, y: 0, width: 750, height: 1624 });
    const doc = await helpers.documentState(page);
    const kinds = {};
    for (const n of Object.values(doc.nodes)) kinds[n.kind] = (kinds[n.kind] || 0) + 1;
    assert.ok(kinds.vector >= 7 && kinds.rectangle >= 10 && !kinds.image,
        "The editor must import editable paths, masks and rectangles without a bitmap substitute");
    const [download] = await Promise.all([
        page.waitForEvent("download", { timeout: 60000 }),
        helpers.clickNative(page, "ExportPngButton", "InspectorViewport"),
    ]);
    assert.equal(await download.failure(), null);
    const stem = resolve(output, mode + "-figma-gid1-11");
    await download.saveAs(stem + ".png");
    const bytes = await readFile(stem + ".png");
    const actual = PNG.sync.read(bytes);
    assert.deepEqual([actual.width, actual.height], [expected.width, expected.height]);
    const pixels = actual.width * actual.height;
    const delta = new PNG({ width: actual.width, height: actual.height });
    const mismatches = new Map();
    const regions = {
        full: { pixels: 0, exact: 0, withinThree: 0, overSixteen: 0, channelError: 0 },
        detail: { pixels: 0, exact: 0, withinThree: 0, overSixteen: 0, channelError: 0 },
    };
    for (let i = 0; i < pixels * 4; i += 4) {
        let max = 0, sum = 0;
        for (let channel = 0; channel < 4; channel++) {
            const difference = Math.abs(actual.data[i + channel] - expected.data[i + channel]);
            max = Math.max(max, difference);
            sum += difference;
        }
        // Exclude the two exact reference background colors for the separate
        // detail metric. The full-image metric still counts every channel.
        const color = expected.data.readUInt32BE(i);
        if (max > 0) {
            const pair = color.toString(16).padStart(8, "0") + " → " +
                actual.data.readUInt32BE(i).toString(16).padStart(8, "0");
            mismatches.set(pair, (mismatches.get(pair) || 0) + 1);
        }
        const detail = color !== 0xffffffff && color !== 0xeaf2ffff;
        for (const area of detail ? [regions.full, regions.detail] : [regions.full]) {
            area.pixels++;
            area.exact += Number(max === 0);
            area.withinThree += Number(max <= 3);
            area.overSixteen += Number(max > 16);
            area.channelError += sum;
        }
        delta.data[i] = delta.data[i + 1] = delta.data[i + 2] = Math.min(255, max * 4);
        delta.data[i + 3] = 255;
    }
    for (const area of Object.values(regions)) {
        area.meanAbsoluteChannelError = area.channelError / (4 * area.pixels);
        area.exactPercent = 100 * area.exact / area.pixels;
        delete area.channelError;
    }
    await writeFile(stem + "-difference.png", PNG.sync.write(delta));
    const result = { dataset: "kixlab/CANVAS@004d47e0938e6f4f30dcec06f6147c15ebdd0c60",
        sample: "replication_gt/gid1-11", node: "1:308", mode, sourceHashes: hashes,
        outputSHA256: sha256(bytes), bounds: view.selectionBounds, kinds, regions,
        mostFrequentMismatch: [...mismatches].sort((a, b) => b[1] - a[1])[0] || null };
    await writeFile(stem + ".json", JSON.stringify(result, null, 2) + "\n");
    check("Same-node Figma artwork comparison at 750×1624: " + regions.full.exact + "/" + pixels +
        " pixels match exactly in RGBA; mean absolute channel error " +
        regions.full.meanAbsoluteChannelError.toFixed(3) + " of 255");
}
