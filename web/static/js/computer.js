// ============================================================================
// Jarvis Computer Use & Emergency Stop Subsystem (Month 5)
// Controls desktop computer use settings, app allow list awareness,
// and emergency stop hotkey / menubar trigger.
// ============================================================================

(function () {
    'use strict';

    let computerStatus = {
        enabled: false,
        stopped: false,
        allowed_apps: ["notepad.exe", "calc.exe", "calculator.exe", "mspaint.exe"],
        stop_hotkey: "Escape",
        ask_all: false,
        active_window: null
    };

    let menubarStopBtn = null;
    let menubarStopIcon = null;
    let menubarStopLabel = null;

    function getJarvisToken() {
        let meta = document.querySelector('meta[name="jarvis-token"]')?.content;
        if (meta && meta !== '{{ jarvis_token }}' && !meta.includes('[REDACTED]')) {
            return meta;
        }
        if (window.location.hash) {
            const match = window.location.hash.match(/[#&]t=([a-f0-9]+)/i);
            if (match) return match[1];
        }
        const search = new URLSearchParams(window.location.search);
        const qTok = search.get('t') || search.get('token');
        if (qTok) return qTok;
        return window.JARVIS_TOKEN || window.__JARVIS_TOKEN__ || (window.oblivion ? window.oblivion.token : '') || '';
    }

    function getAuthHeaders(extra = {}) {
        const token = getJarvisToken();
        const headers = { ...extra };
        if (token) {
            headers['X-Jarvis-Token'] = token;
            headers['x-oblivion-token'] = token;
        }
        return headers;
    }

    async function fetchStatus() {
        try {
            const token = getJarvisToken();
            const url = '/api/oblivion/computer/status' + (token ? '?t=' + encodeURIComponent(token) : '');
            const res = await fetch(url, {
                headers: getAuthHeaders()
            });
            if (res.ok) {
                const data = await res.json();
                computerStatus = Object.assign(computerStatus, data);
                updateUI();
            }
        } catch (e) {
            console.warn('[ComputerUse] Failed to fetch status:', e);
        }
    }

    async function triggerStop() {
        // Optimistic instant emergency stop
        computerStatus.stopped = true;
        updateUI();

        try {
            const token = getJarvisToken();
            const url = '/api/oblivion/computer/stop' + (token ? '?t=' + encodeURIComponent(token) : '');
            const res = await fetch(url, {
                method: 'POST',
                headers: getAuthHeaders({ 'Content-Type': 'application/json' })
            });
            if (res.ok) {
                const data = await res.json();
                computerStatus.stopped = true;
                updateUI();
                console.log('[ComputerUse] Emergency stop triggered:', data.message);
            }
        } catch (e) {
            console.error('[ComputerUse] Failed to trigger emergency stop:', e);
            computerStatus.stopped = true;
            updateUI();
        }

        // Cancel any running turn in Oblivion if active
        if (window.oblivion && typeof window.oblivion.cancel === 'function') {
            window.oblivion.cancel();
        }
    }

    async function resetStop() {
        // Optimistic reset
        computerStatus.stopped = false;
        updateUI();

        try {
            const token = getJarvisToken();
            const url = '/api/oblivion/computer/reset' + (token ? '?t=' + encodeURIComponent(token) : '');
            const res = await fetch(url, {
                method: 'POST',
                headers: getAuthHeaders({ 'Content-Type': 'application/json' })
            });
            if (res.ok) {
                const data = await res.json();
                computerStatus.stopped = false;
                updateUI();
                console.log('[ComputerUse] Emergency stop reset:', data.message);
            }
        } catch (e) {
            console.error('[ComputerUse] Failed to reset emergency stop:', e);
        }
    }

    function updateUI() {
        if (!menubarStopBtn) return;

        if (computerStatus.stopped) {
            menubarStopBtn.classList.add('stopped-active');
            if (menubarStopIcon) menubarStopIcon.textContent = '⛔';
            if (menubarStopLabel) menubarStopLabel.textContent = 'STOPPED (Resume)';
            menubarStopBtn.title = 'Emergency Stop Active: Desktop operations halted. Click to resume.';
        } else {
            menubarStopBtn.classList.remove('stopped-active');
            if (menubarStopIcon) menubarStopIcon.textContent = '🛑';
            if (menubarStopLabel) menubarStopLabel.textContent = 'Stop (Esc)';
            menubarStopBtn.title = 'Emergency Stop (Esc): Click to immediately halt desktop computer actions';
        }
    }

    function init() {
        menubarStopBtn = document.getElementById('menubar-stop-btn');
        menubarStopIcon = document.getElementById('menubar-stop-icon');
        menubarStopLabel = document.getElementById('menubar-stop-label');

        if (menubarStopBtn) {
            menubarStopBtn.addEventListener('click', (ev) => {
                ev.preventDefault();
                if (computerStatus.stopped) {
                    resetStop();
                } else {
                    triggerStop();
                }
            });
        }

        // Hard stop hotkey listener (Escape key anywhere)
        window.addEventListener('keydown', (ev) => {
            if (ev.key === 'Escape') {
                // If Oblivion input has open suggestions or modals, let them handle first,
                // but if computer use operations or agent turn is busy, trigger stop.
                if (computerStatus.enabled || document.querySelector('.ob-tool.run')) {
                    triggerStop();
                }
            }
        }, true);

        fetchStatus();
    }

    window.JarvisComputer = {
        fetchStatus,
        triggerStop,
        resetStop,
        getStatus: () => Object.assign({}, computerStatus)
    };

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }
})();
