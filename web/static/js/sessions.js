/**
 * Jarvis Desktop - Sessions & Workspaces Manager (B10)
 * Handles listing, searching, renaming, resuming, deleting, and setting work folder per session.
 */
(function () {
    "use strict";

    let modal = null;
    let itemsContainer = null;
    let searchInput = null;
    let countBadge = null;
    let newBtn = null;
    let editorPane = null;
    let editorForm = null;
    let workspaceInput = null;
    let closeEditorBtn = null;
    let cancelEditorBtn = null;
    let searchTimer = null;
    let activeSessionId = null;

    function getToken() {
        return (
            window.JARVIS_TOKEN ||
            document.querySelector('meta[name="jarvis-token"]')?.getAttribute("content") ||
            ""
        );
    }

    function authHeaders() {
        const headers = { "Content-Type": "application/json" };
        const token = getToken();
        if (token) headers["X-Jarvis-Token"] = token;
        return headers;
    }

    function formatTime(isoStr) {
        if (!isoStr) return "";
        try {
            const d = new Date(isoStr);
            if (isNaN(d.getTime())) return isoStr;
            return d.toLocaleString(undefined, {
                month: "short",
                day: "numeric",
                hour: "2-digit",
                minute: "2-digit",
            });
        } catch {
            return isoStr;
        }
    }

    function escapeHtml(text) {
        if (!text) return "";
        const div = document.createElement("div");
        div.textContent = text;
        return div.innerHTML;
    }

    function openSessions() {
        if (!modal) return;
        modal.classList.add("active");
        modal.style.display = "flex";
        loadSessions();
        if (searchInput) searchInput.focus();
    }

    function closeSessions() {
        if (!modal) return;
        modal.classList.remove("active");
        modal.style.display = "none";
        hideNewSessionDrawer();
    }

    function toggleSessions() {
        if (!modal) return;
        if (modal.classList.contains("active") || modal.style.display === "flex") {
            closeSessions();
        } else {
            openSessions();
        }
    }

    function showNewSessionDrawer() {
        if (!editorPane) return;
        editorPane.style.display = "block";
        if (workspaceInput && !workspaceInput.value) {
            workspaceInput.value = "D:\\Jarvis";
        }
        if (workspaceInput) workspaceInput.focus();
    }

    function hideNewSessionDrawer() {
        if (!editorPane) return;
        editorPane.style.display = "none";
    }

    async function loadSessions(query) {
        if (!itemsContainer) return;
        itemsContainer.innerHTML = `
            <div class="sessions-empty-state">
                <div class="empty-icon">🗂️</div>
                <div class="empty-text">Loading sessions...</div>
            </div>
        `;

        try {
            // Determine active session if available
            try {
                const stRes = await fetch("/api/oblivion/status", { headers: authHeaders() });
                if (stRes.ok) {
                    const st = await stRes.json();
                    activeSessionId = st.session || activeSessionId;
                }
            } catch {
                // status check is advisory
            }

            let url = "/api/oblivion/sessions";
            if (query && query.trim()) {
                url += `?q=${encodeURIComponent(query.trim())}`;
            }

            const res = await fetch(url, { headers: authHeaders() });
            if (!res.ok) {
                const errText = await res.text();
                itemsContainer.innerHTML = `
                    <div class="sessions-empty-state">
                        <div class="empty-icon">⚠️</div>
                        <div class="empty-text">Could not load sessions: ${escapeHtml(errText)}</div>
                    </div>
                `;
                return;
            }

            const sessions = await res.json();
            renderSessions(sessions);
        } catch (err) {
            itemsContainer.innerHTML = `
                <div class="sessions-empty-state">
                    <div class="empty-icon">⚠️</div>
                    <div class="empty-text">Error: ${escapeHtml(err.message)}</div>
                </div>
            `;
        }
    }

    function renderSessions(sessions) {
        if (!itemsContainer) return;
        if (!Array.isArray(sessions) || sessions.length === 0) {
            if (countBadge) countBadge.textContent = "0 sessions";
            itemsContainer.innerHTML = `
                <div class="sessions-empty-state">
                    <div class="empty-icon">📭</div>
                    <div class="empty-text">No matching sessions found</div>
                </div>
            `;
            return;
        }

        if (countBadge) countBadge.textContent = `${sessions.length} session${sessions.length === 1 ? "" : "s"}`;
        itemsContainer.innerHTML = "";

        sessions.forEach((s) => {
            const card = document.createElement("div");
            card.className = "session-card";
            card.dataset.sessionId = s.id;

            const isActive = activeSessionId && activeSessionId === s.id;
            if (isActive) card.classList.add("active-session");

            const title = s.first_prompt || "Untitled Session";
            const timeStr = formatTime(s.updated || s.created);
            const msgCount = typeof s.message_count === "number" ? s.message_count : 0;
            const ws = s.workspace || "D:\\Jarvis";

            card.innerHTML = `
                <div class="session-card-header">
                    <div class="session-title-group">
                        <span class="session-id-pill" title="Session ID">${escapeHtml(s.id)}</span>
                        <span class="session-title-text" title="${escapeHtml(title)}">${escapeHtml(title)}</span>
                    </div>
                    ${isActive ? '<span class="session-active-badge">Active</span>' : ""}
                    <span class="session-time">${escapeHtml(timeStr)}</span>
                </div>
                <div class="session-card-details">
                    <span class="session-workspace-path" title="Work folder: ${escapeHtml(ws)}">📁 ${escapeHtml(ws)}</span>
                    <span class="session-meta-pill" style="opacity: 0.7;">💬 ${msgCount} msgs</span>
                </div>
                <div class="session-card-actions">
                    <button class="btn-change-folder" data-id="${escapeHtml(s.id)}" data-ws="${escapeHtml(ws)}" title="Change work folder (B10)">📁 Change Folder</button>
                    <button class="btn-rename" data-id="${escapeHtml(s.id)}" data-title="${escapeHtml(title)}" title="Rename session">✏️ Rename</button>
                    <button class="btn-delete" data-id="${escapeHtml(s.id)}" title="Delete session">🗑️ Delete</button>
                    <button class="btn-resume" data-id="${escapeHtml(s.id)}" title="Resume this session">▶ Resume</button>
                </div>
            `;

            // Action: Resume
            card.querySelector(".btn-resume").addEventListener("click", () => {
                resumeSession(s.id);
            });

            // Action: Rename
            card.querySelector(".btn-rename").addEventListener("click", () => {
                renameSession(s.id, title);
            });

            // Action: Change Folder (B10)
            card.querySelector(".btn-change-folder").addEventListener("click", () => {
                changeWorkspace(s.id, ws);
            });

            // Action: Delete
            card.querySelector(".btn-delete").addEventListener("click", () => {
                deleteSession(s.id);
            });

            itemsContainer.appendChild(card);
        });
    }

    async function resumeSession(id) {
        try {
            const res = await fetch(`/api/oblivion/sessions/${encodeURIComponent(id)}/resume`, {
                method: "POST",
                headers: authHeaders(),
                body: JSON.stringify({}),
            });
            if (res.ok) {
                activeSessionId = id;
                const badge = document.getElementById("ob-active-session-badge");
                if (badge) badge.textContent = `SESSION: ${id}`;
                closeSessions();
                // If Oblivion chat is available, trigger a UI update
                if (window.JarvisOblivion && typeof window.JarvisOblivion.open === "function") {
                    window.JarvisOblivion.open();
                }
            } else {
                const err = await res.text();
                alert(`Could not resume session: ${err}`);
            }
        } catch (err) {
            alert(`Error resuming session: ${err.message}`);
        }
    }

    async function renameSession(id, currentTitle) {
        const newTitle = prompt("Enter new title for session:", currentTitle);
        if (newTitle === null) return;
        const trimmed = newTitle.trim();
        if (!trimmed) return;

        try {
            const res = await fetch(`/api/oblivion/sessions/${encodeURIComponent(id)}/rename`, {
                method: "POST",
                headers: authHeaders(),
                body: JSON.stringify({ title: trimmed }),
            });
            if (res.ok) {
                loadSessions(searchInput?.value);
            } else {
                const err = await res.text();
                alert(`Could not rename session: ${err}`);
            }
        } catch (err) {
            alert(`Error renaming session: ${err.message}`);
        }
    }

    async function changeWorkspace(id, currentWs) {
        const newWs = prompt("Enter new work folder path for this session (B10):", currentWs);
        if (newWs === null) return;
        const trimmed = newWs.trim();
        if (!trimmed) return;

        try {
            const res = await fetch(`/api/oblivion/sessions/${encodeURIComponent(id)}/workspace`, {
                method: "POST",
                headers: authHeaders(),
                body: JSON.stringify({ workspace: trimmed }),
            });
            if (res.ok) {
                loadSessions(searchInput?.value);
            } else {
                const err = await res.text();
                alert(`Could not change work folder: ${err}`);
            }
        } catch (err) {
            alert(`Error changing work folder: ${err.message}`);
        }
    }

    async function deleteSession(id) {
        if (!confirm(`Are you sure you want to delete session ${id}? This cannot be undone.`)) {
            return;
        }

        try {
            const res = await fetch(`/api/oblivion/sessions/${encodeURIComponent(id)}`, {
                method: "DELETE",
                headers: authHeaders(),
            });
            if (res.ok) {
                loadSessions(searchInput?.value);
            } else {
                const err = await res.text();
                alert(`Could not delete session: ${err}`);
            }
        } catch (err) {
            alert(`Error deleting session: ${err.message}`);
        }
    }

    async function handleNewSessionSubmit(e) {
        if (e) e.preventDefault();
        const ws = (workspaceInput?.value || "").trim();
        if (!ws) {
            alert("Please provide a work folder path.");
            return;
        }

        try {
            const res = await fetch("/api/oblivion/sessions", {
                method: "POST",
                headers: authHeaders(),
                body: JSON.stringify({ workspace: ws }),
            });
            if (res.ok) {
                hideNewSessionDrawer();
                closeSessions();
                // Open chat space
                if (window.JarvisOblivion && typeof window.JarvisOblivion.open === "function") {
                    window.JarvisOblivion.open();
                }
            } else {
                const err = await res.text();
                alert(`Could not create session: ${err}`);
            }
        } catch (err) {
            alert(`Error creating session: ${err.message}`);
        }
    }

    function init() {
        modal = document.getElementById("sessionsModal");
        itemsContainer = document.getElementById("sessions-items-container");
        searchInput = document.getElementById("sessions-search-input");
        countBadge = document.getElementById("sessions-count-badge");
        newBtn = document.getElementById("sessions-new-btn");
        editorPane = document.getElementById("sessions-editor-pane");
        editorForm = document.getElementById("sessions-editor-form");
        workspaceInput = document.getElementById("new-session-workspace");
        closeEditorBtn = document.getElementById("sessions-editor-close-btn");
        cancelEditorBtn = document.getElementById("sessions-editor-cancel-btn");

        // Close button
        const closeBtn = document.getElementById("close-sessions-modal-btn");
        if (closeBtn) closeBtn.addEventListener("click", closeSessions);

        // Backdrop click
        if (modal) {
            modal.addEventListener("click", (e) => {
                if (e.target === modal) closeSessions();
            });
        }

        // Dock button
        const dockBtn = document.getElementById("open-sessions-dock-btn");
        if (dockBtn) {
            dockBtn.addEventListener("click", (e) => {
                e.preventDefault();
                toggleSessions();
            });
        }

        // Active session badge click
        const activeBadge = document.getElementById("ob-active-session-badge");
        if (activeBadge) {
            activeBadge.addEventListener("click", (e) => {
                e.preventDefault();
                openSessions();
            });
        }

        // Search debounce
        if (searchInput) {
            searchInput.addEventListener("input", () => {
                clearTimeout(searchTimer);
                searchTimer = setTimeout(() => {
                    loadSessions(searchInput.value);
                }, 200);
            });
        }

        // New Session button & drawer
        if (newBtn) newBtn.addEventListener("click", showNewSessionDrawer);
        if (closeEditorBtn) closeEditorBtn.addEventListener("click", hideNewSessionDrawer);
        if (cancelEditorBtn) cancelEditorBtn.addEventListener("click", hideNewSessionDrawer);
        if (editorForm) editorForm.addEventListener("submit", handleNewSessionSubmit);

        // Global hotkey: Ctrl+S or Alt+S
        document.addEventListener("keydown", (e) => {
            if ((e.ctrlKey && e.key.toLowerCase() === "s") || (e.altKey && e.key.toLowerCase() === "s")) {
                e.preventDefault();
                toggleSessions();
            } else if (e.key === "Escape" && modal && modal.classList.contains("active")) {
                closeSessions();
            }
        });
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
