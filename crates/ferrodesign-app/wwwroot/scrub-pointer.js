// Browser capability for the native inspector. FerroUI continues to own hit
// testing, pointer capture, modifiers, sampling and undo on the ordinary route.
export function installScrubPointer(root, cancelNative) {
    const doc = root.ownerDocument;
    const win = doc.defaultView;
    let sequence = 0;
    let pressed = null;
    let active = null;
    let pending = null;
    let owner = null;

    function release() {
        if (doc.pointerLockElement !== root) return;
        try { doc.exitPointerLock(); }
        catch { /* A document being detached may already be relinquishing its lock. */ }
    }

    function end(token) {
        if (!active || active.token !== token) return;
        const previous = active;
        active = null;
        if (owner === previous) {
            owner = null;
            release();
        } else if (pending === previous && doc.pointerLockElement === root) {
            pending = null;
            release();
        }
        // A request can be granted after release. Keep its identity until the
        // browser answers so a late grant cannot be adopted by the next drag.
    }

    function cancel() {
        if (!active) return;
        const token = active.token;
        end(token);
        cancelNative(token);
    }

    function denied(attempt) {
        if (pending !== attempt) return;
        pending = null;
        if (active === attempt && attempt.captureLost && !root.hasPointerCapture?.(attempt.press.pointerId)) cancel();
    }

    function synchronize() {
        if (doc.pointerLockElement === root) {
            if (!pending) return;
            const grant = pending;
            pending = null;
            if (active === grant && pressed === grant.press && !doc.hidden) {
                owner = grant;
            } else {
                release();
            }
        } else if (owner) {
            // Browser Escape can exit pointer lock without delivering KeyDown.
            owner = null;
            cancel();
        }
    }

    root.addEventListener("pointerdown", e => {
        if (e.isTrusted && e.pointerType === "mouse" && e.isPrimary && e.button === 0) {
            pressed = { pointerId: e.pointerId };
        }
    }, true);

    root.addEventListener("pointermove", e => {
        if (!active || !e.isTrusted || e.pointerType !== "mouse" || e.pointerId !== active.press.pointerId) return;
        if (owner || doc.pointerLockElement === root) synchronize();
        // Some hosts emit cursor-recentering events with no buttons down.
        // Only actual movement of the held primary mouse belongs to this edit.
        if (!owner || !(e.buttons & 1)) return;
        // PointerEvent movement is the displacement of the aggregate event.
        // Summing its coalesced children as well would count the motion twice.
        if (Number.isFinite(e.movementX)) owner.x += e.movementX;
        if (Number.isFinite(e.movementY)) owner.y += e.movementY;
    }, true);

    win.addEventListener("pointerup", e => {
        if (!pressed || e.pointerId !== pressed.pointerId || e.button !== 0) return;
        const released = pressed;
        if (owner && doc.pointerLockElement !== root) synchronize();
        // FerroUI dispatches pointer-up synchronously at the host element. It
        // gets the final virtual coordinate and commits before this safety net.
        // A microtask can run between capture and bubble listeners on real input.
        setTimeout(() => {
            if (pressed === released) pressed = null;
            if (active?.press === released) cancel();
        }, 0);
    }, true);
    win.addEventListener("pointercancel", e => {
        if (pressed?.pointerId === e.pointerId) {
            pressed = null;
            cancel();
        }
    }, true);
    root.addEventListener("lostpointercapture", e => {
        if (e.target !== root || active?.press.pointerId !== e.pointerId || root.hasPointerCapture?.(e.pointerId)) return;
        // Chromium can release DOM capture before publishing pointerLockElement.
        // Wait for this request's result to distinguish a grant from actual loss.
        if (doc.pointerLockElement === root) return;
        if (pending === active) active.captureLost = true;
        else cancel();
    });
    const interrupt = () => { pressed = null; cancel(); };
    win.addEventListener("blur", interrupt);
    win.addEventListener("pagehide", interrupt, true);
    doc.addEventListener("visibilitychange", () => { if (doc.hidden) interrupt(); }, true);
    doc.addEventListener("pointerlockchange", synchronize);
    doc.addEventListener("pointerlockerror", () => {
        // Promise-based requests have an identity-bound rejection handler.
        if (pending?.legacy) denied(pending);
    });

    return {
        arm(x, y) {
            if (active || !pressed || doc.hidden || doc.pointerLockElement) return 0;
            sequence = (sequence + 1) >>> 0 || 1;
            active = { token: sequence, press: pressed, x, y, legacy: false };
            return sequence;
        },
        request(token) {
            if (!active || token !== active.token || active.press !== pressed || pending || owner ||
                doc.hidden || doc.pointerLockElement || typeof root.requestPointerLock !== "function" ||
                win.navigator.userActivation?.isActive === false) return;
            const attempt = active;
            pending = attempt;
            try {
                const result = root.requestPointerLock();
                if (typeof result?.then === "function") {
                    result.then(() => { if (pending === attempt) synchronize(); },
                        () => denied(attempt));
                } else {
                    attempt.legacy = true;
                }
            } catch {
                denied(attempt);
            }
        },
        coordinate(token, axis, absolute) {
            if (!active || active.token !== token) return absolute;
            const key = axis === 0 ? "x" : "y";
            // While a grant is pending, native positions remain authoritative.
            // The first relative sample starts from the last bounded sample.
            if (owner !== active || doc.pointerLockElement !== root) active[key] = absolute;
            return active[key];
        },
        end,
    };
}
