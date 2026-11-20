// Jarvis OS behaviour layer (pairs with static/css/glass.css).
//
// - Dock: icons grow under the pointer like the macOS dock, bounce when they
//   open something, and show a dot while their window or panel is open.
// - Windows: the yellow and green traffic lights minimise and maximise.
// - Lock screen: the clock and date are live.
//
// Only reads ids and classes the other scripts already own, and never calls
// their functions, so it can be removed without breaking anything.

(function () {
    "use strict";

    const MAX_SCALE = 1.6;      // size of the icon right under the pointer
    const REACH = 130;          // px: how far the magnification spreads

    // Which window each dock icon opens. The workspace window serves several icons,
    // so for those the dot follows the icon that was clicked last.
    const WINDOW_FOR = {
        memory: "memoryModal",
        sessions: "sessionsModal",
        models: "modelsModal",
        files: "filesModal",
        terminal: "splitModal",
        web: "splitModal",
        skills: "splitModal",
        mcp: "splitModal",
    };

    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let lastSplitAction = null;

    function $(id) { return document.getElementById(id); }

    // ---------- dock magnification ----------
    function initMagnify(row) {
        const btns = Array.from(row.querySelectorAll(".dock-app-btn"));
        if (!btns.length || reduceMotion) return;
        let frame = 0;
        let pointerX = null;

        function apply() {
            frame = 0;
            for (const b of btns) {
                let s = 1;
                if (pointerX !== null) {
                    const r = b.getBoundingClientRect();
                    const d = Math.abs(pointerX - (r.left + r.width / 2));
                    if (d < REACH) {
                        // Smooth bell curve: full size at the pointer, back to 1 at REACH.
                        const t = Math.cos((d / REACH) * (Math.PI / 2));
                        s = 1 + (MAX_SCALE - 1) * t * t;
                    }
                }
                b.style.setProperty("--s", s.toFixed(3));
            }
        }
        row.addEventListener("mousemove", (e) => {
            pointerX = e.clientX;
            if (!frame) frame = requestAnimationFrame(apply);
        });
        row.addEventListener("mouseleave", () => {
            pointerX = null;
            if (!frame) frame = requestAnimationFrame(apply);
        });
    }

    // ---------- running dots and bounce ----------
    function isOpen(id) {
        const el = $(id);
        return !!el && el.classList.contains("active");
    }

    function refreshDots() {
        document.querySelectorAll(".dock-app-btn").forEach((b) => {
            let running = false;
            if (b.id === "open-chat-dock-btn") {
                running = !!$("ob-panel")?.classList.contains("open");
            } else {
                const win = WINDOW_FOR[b.dataset.action];
                if (win === "splitModal") running = isOpen(win) && lastSplitAction === b.dataset.action;
                else if (win) running = isOpen(win);
            }
            b.classList.toggle("running", running);
        });
    }

    function bounce(btn) {
        if (reduceMotion || btn.classList.contains("running")) return;
        btn.classList.remove("bounce");
        void btn.offsetWidth; // restart the animation
        btn.classList.add("bounce");
        setTimeout(() => btn.classList.remove("bounce"), 650);
    }

    function initDots() {
        document.querySelectorAll(".dock-app-btn").forEach((b) => {
            // Capture phase: runs before the other scripts open the window.
            b.addEventListener("click", () => {
                if (WINDOW_FOR[b.dataset.action] === "splitModal") lastSplitAction = b.dataset.action;
                bounce(b);
            }, true);
        });
        const watched = ["memoryModal", "sessionsModal", "modelsModal", "filesModal", "splitModal", "ob-panel"]
            .map($)
            .filter(Boolean);
        const obs = new MutationObserver(refreshDots);
        watched.forEach((el) => obs.observe(el, { attributes: true, attributeFilter: ["class"] }));
        refreshDots();
    }

    // ---------- traffic lights ----------
    function initTrafficLights() {
        document.querySelectorAll(".split-modal-overlay").forEach((overlay) => {
            const frame = overlay.querySelector(".split-window-frame");
            const min = overlay.querySelector(".tl-min");
            const max = overlay.querySelector(".tl-max");
            const header = overlay.querySelector(".split-frame-header");
            if (min) {
                min.addEventListener("click", (e) => {
                    e.stopPropagation();
                    overlay.classList.remove("active");
                });
            }
            const toggleMax = (e) => {
                e.stopPropagation();
                frame?.classList.toggle("maximized");
            };
            if (max) max.addEventListener("click", toggleMax);
            // Double-clicking the title bar zooms, as on macOS.
            if (header) {
                header.addEventListener("dblclick", (e) => {
                    if (e.target.closest("button, input, select, a")) return;
                    toggleMax(e);
                });
            }
        });
    }

    // ---------- lock screen clock ----------
    function initLockClock() {
        const time = $("lock-time");
        const date = $("lock-date");
        if (!time && !date) return;
        const tick = () => {
            const now = new Date();
            if (time) time.textContent = now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }).replace(/\s?[AP]M$/i, "");
            if (date) date.textContent = now.toLocaleDateString([], { weekday: "long", month: "long", day: "numeric" });
        };
        tick();
        setInterval(tick, 1000);
    }

    // ---------- keyboard unlock ----------
    function initUnlockKeys() {
        const loader = $("loader");
        if (!loader) return;
        document.addEventListener("keydown", (e) => {
            if (loader.classList.contains("hide")) return;
            if (e.key === "Enter" || e.key === " ") $("unlock-btn")?.click();
        });
    }

    document.addEventListener("DOMContentLoaded", () => {
        const row = document.querySelector(".dock-icons-row");
        if (row) initMagnify(row);
        initDots();
        initTrafficLights();
        initLockClock();
        initUnlockKeys();
    });
})();
