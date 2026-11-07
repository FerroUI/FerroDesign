import assert from "node:assert/strict";
import { test } from "node:test";
import { installScrubPointer } from "../crates/ferrodesign-app/wwwroot/scrub-pointer.js";

class Events {
    listeners = new Map();
    addEventListener(name, callback) {
        const list = this.listeners.get(name) || [];
        this.listeners.set(name, [...list, callback]);
    }
    send(name, props = {}) {
        for (const cb of this.listeners.get(name) || []) {
            cb({ target: this, isTrusted: true, pointerType: "mouse", pointerId: 1, isPrimary: true, button: 0, buttons: 1, ...props });
        }
    }
}

function fixture(legacy = false) {
    const root = new Events(), doc = new Events(), win = new Events();
    root.ownerDocument = doc;
    doc.defaultView = win;
    win.navigator = { userActivation: { isActive: true } };
    const canceled = [], attempts = [];
    let exits = 0;
    doc.exitPointerLock = () => {
        exits++;
        doc.pointerLockElement = null;
        doc.send("pointerlockchange");
    };
    root.requestPointerLock = () => {
        let resolve, reject;
        const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
        attempts.push({ resolve, reject });
        return legacy ? undefined : promise;
    };
    const api = installScrubPointer(root, n => canceled.push(n));
    const arm = (props = {}) => { root.send("pointerdown", props); return api.arm(10.125, 20.5); };
    const grant = () => { doc.pointerLockElement = root; doc.send("pointerlockchange"); };
    const move = (dx, dy) => root.send("pointermove", { movementX: dx, movementY: dy });
    return { root, doc, win, canceled, attempts, exits: () => exits, api, arm, grant, move };
}

test("a trusted primary mouse press arms, but a click, touch, or expired activation never requests a lock", () => {
    const f = fixture();
    assert.equal(f.api.arm(1, 1), 0);
    for (const props of [{ pointerType: "touch" }, { isPrimary: false }, { isTrusted: false }, { button: 2 }]) {
        assert.equal(f.arm(props), 0);
    }
    const click = f.arm();
    assert.ok(click);
    f.api.end(click);
    assert.equal(f.attempts.length, 0);
    const drag = f.arm();
    f.win.navigator.userActivation.isActive = false;
    f.api.request(drag);
    assert.equal(f.attempts.length, 0);
    assert.equal(f.api.coordinate(drag, 0, 32.5), 32.5);
    assert.deepEqual(f.canceled, []);
});

test("pending acquisition uses bounded positions, then relative deltas continue from the last sample", async () => {
    const f = fixture(), token = f.arm();
    f.api.request(token);
    assert.equal(f.api.coordinate(token, 0, 32.125), 32.125);
    assert.equal(f.api.coordinate(token, 1, 41.5), 41.5);
    f.move(40, 40); // not locked yet
    f.grant();
    f.attempts[0].resolve();
    await Promise.resolve();
    f.root.send("lostpointercapture");
    f.move(1800, -50);
    f.root.send("pointermove", { buttons: 0, movementX: -1800, movementY: 50 });
    f.move(-24, 4);
    assert.equal(f.api.coordinate(token, 0, 32), 1808.125);
    assert.equal(f.api.coordinate(token, 1, 41), -4.5);
    f.win.send("pointerup");
    // The framework's real release handler runs before the next task.
    f.api.end(token);
    await Promise.resolve();
    assert.equal(f.exits(), 1);
    assert.deepEqual(f.canceled, []);
});

test("release before grant relinquishes a stale lock without stealing a later or foreign session", async () => {
    const f = fixture(), first = f.arm();
    f.api.request(first);
    f.api.end(first);
    const second = f.arm();
    f.api.request(second);
    assert.equal(f.attempts.length, 1);
    f.grant();
    assert.equal(f.exits(), 1);
    assert.equal(f.api.coordinate(second, 0, 48), 48);
    f.attempts[0].resolve();
    await Promise.resolve();
    f.api.end(second);
    assert.deepEqual(f.canceled, []);
    f.doc.pointerLockElement = {};
    assert.equal(f.arm(), 0);
    f.win.send("blur");
    assert.equal(f.exits(), 1);
});

test("a rejected, unsupported, legacy-failed or throwing request keeps bounded editing available", async () => {
    for (const kind of ["promise", "legacy", "throws", "unsupported"]) {
        const f = fixture(kind === "legacy");
        if (kind === "unsupported") delete f.root.requestPointerLock;
        if (kind === "throws") f.root.requestPointerLock = () => { throw new Error("Denied"); };
        const token = f.arm();
        assert.ok(token, "bounded gestures still participate in cancellation");
        f.api.request(token);
        if (kind === "promise") { f.attempts[0].reject(new Error("Denied")); await Promise.resolve(); }
        if (kind === "legacy") f.doc.send("pointerlockerror");
        assert.equal(f.api.coordinate(token, 0, 77), 77);
        assert.deepEqual(f.canceled, []);
        f.api.end(token);
    }
});

test("default browser unlock, capture loss, cancellation, blur and page hiding cancel only their owner", async () => {
    for (const reason of ["unlock", "capture", "cancel", "blur", "hidden", "pagehide", "missedUp"]) {
        const f = fixture(true), token = f.arm();
        if (reason !== "capture") {
            f.api.request(token);
            f.grant();
        }
        if (reason === "unlock") { f.doc.pointerLockElement = null; f.doc.send("pointerlockchange"); }
        if (reason === "capture") f.root.send("lostpointercapture");
        if (reason === "cancel") f.win.send("pointercancel");
        if (reason === "blur") f.win.send("blur");
        if (reason === "hidden") { f.doc.hidden = true; f.doc.send("visibilitychange"); }
        if (reason === "pagehide") f.win.send("pagehide");
        if (reason === "missedUp") { f.win.send("pointerup"); await new Promise(resolve => setTimeout(resolve, 0)); }
        assert.deepEqual(f.canceled, [token], reason);
        f.doc.send("pointerlockchange");
        assert.deepEqual(f.canceled, [token], reason);
    }
});

test("a stale DOM capture-loss event cannot cancel a recaptured mouse", () => {
    const f = fixture(), token = f.arm();
    f.root.hasPointerCapture = () => true;
    f.root.send("lostpointercapture");
    f.root.send("lostpointercapture", { target: {} });
    assert.deepEqual(f.canceled, []);
    f.root.hasPointerCapture = () => false;
    f.root.send("lostpointercapture");
    assert.deepEqual(f.canceled, [token]);
});


test("capture may transfer before Chromium publishes the pointer-lock grant", async () => {
    for (const legacy of [false, true]) {
        for (const accepted of [false, true]) {
            const f = fixture(legacy), token = f.arm();
            f.api.request(token);
            f.root.send("lostpointercapture", { buttons: 0 });
            assert.deepEqual(f.canceled, []);
            if (accepted) {
                f.grant();
                f.move(8, 0);
                assert.equal(f.api.coordinate(token, 0, 10), 18.125);
                assert.deepEqual(f.canceled, []);
                f.api.end(token);
            } else {
                if (legacy) f.doc.send("pointerlockerror");
                else f.attempts[0].reject(new Error("Rejected"));
                await Promise.resolve();
                assert.deepEqual(f.canceled, [token]);
            }
        }
    }
});
