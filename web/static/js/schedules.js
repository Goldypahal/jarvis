// Jarvis Desktop Schedules & Automations Window Controller
// Interacts with /api/oblivion/schedules endpoints for creating, listing, running, pausing, and editing automations.

(function () {
    let currentSchedules = [];
    let isEditorOpen = false;

    function getToken() {
        const meta = document.querySelector('meta[name="jarvis-token"]');
        return meta ? meta.getAttribute('content') : '';
    }

    function authHeaders() {
        return {
            'Content-Type': 'application/json',
            'X-Jarvis-Token': getToken()
        };
    }

    function initSchedulesWindow() {
        const modal = document.getElementById('schedulesModal');
        const openDockBtn = document.getElementById('open-schedules-dock-btn');
        const closeBtn = document.getElementById('close-schedules-modal-btn');
        const newBtn = document.getElementById('schedules-new-btn');
        const editorCloseBtn = document.getElementById('schedules-editor-close-btn');
        const editorCancelBtn = document.getElementById('schedules-editor-cancel-btn');
        const editorForm = document.getElementById('schedules-editor-form');
        const presetSelect = document.getElementById('schedule-preset-select');

        // Toggle / Open
        if (openDockBtn) {
            openDockBtn.addEventListener('click', () => {
                toggleSchedulesModal();
            });
        }

        if (closeBtn && modal) {
            closeBtn.addEventListener('click', () => {
                closeSchedulesModal();
            });
        }

        if (modal) {
            modal.addEventListener('click', (e) => {
                if (e.target === modal) {
                    closeSchedulesModal();
                }
            });
        }

        // Global keyboard shortcut: Alt+A (Automations)
        document.addEventListener('keydown', (e) => {
            if (e.altKey && e.key.toLowerCase() === 'a') {
                e.preventDefault();
                toggleSchedulesModal();
            } else if (e.key === 'Escape' && modal && modal.classList.contains('active')) {
                closeSchedulesModal();
            }
        });

        // New schedule button
        if (newBtn) {
            newBtn.addEventListener('click', () => {
                openEditor();
            });
        }

        if (editorCloseBtn) {
            editorCloseBtn.addEventListener('click', closeEditor);
        }
        if (editorCancelBtn) {
            editorCancelBtn.addEventListener('click', closeEditor);
        }

        // Preset selector auto-fill
        if (presetSelect) {
            presetSelect.addEventListener('change', (e) => {
                const val = e.target.value;
                if (val) {
                    const exprInput = document.getElementById('edit-schedule-expr');
                    if (exprInput) {
                        exprInput.value = val;
                    }
                }
            });
        }

        // Form Submit (Create or Update)
        if (editorForm) {
            editorForm.addEventListener('submit', async (e) => {
                e.preventDefault();
                await handleSaveSchedule();
            });
        }

        // Starters Seed All Button
        const seedAllBtn = document.getElementById('schedules-starters-seed-all-btn');
        if (seedAllBtn) {
            seedAllBtn.addEventListener('click', async () => {
                seedAllBtn.disabled = true;
                seedAllBtn.textContent = '⏳ Adding All...';
                try {
                    const startersRes = await fetch('/api/oblivion/schedules/starters', { headers: authHeaders() });
                    if (startersRes.ok) {
                        const starters = await startersRes.json();
                        for (const s of starters) {
                            if (!s.installed) {
                                await fetch(`/api/oblivion/schedules/starters/${encodeURIComponent(s.id)}/install`, {
                                    method: 'POST',
                                    headers: authHeaders()
                                });
                            }
                        }
                    }
                    await loadStarterAutomations();
                    await loadSchedules();
                } catch (err) {
                    console.error('Failed to seed all starters:', err);
                } finally {
                    seedAllBtn.disabled = false;
                    seedAllBtn.textContent = '⚡ Add All Starters';
                }
            });
        }
    }

    function toggleSchedulesModal() {
        const modal = document.getElementById('schedulesModal');
        if (!modal) return;
        if (modal.classList.contains('active')) {
            closeSchedulesModal();
        } else {
            openSchedulesModal();
        }
    }

    function openSchedulesModal() {
        const modal = document.getElementById('schedulesModal');
        if (!modal) return;
        modal.classList.add('active');
        loadSchedules();
    }

    function closeSchedulesModal() {
        const modal = document.getElementById('schedulesModal');
        if (!modal) return;
        modal.classList.remove('active');
        closeEditor();
    }

    function openEditor(task = null) {
        const pane = document.getElementById('schedules-editor-pane');
        const title = document.getElementById('schedules-editor-title');
        const idInput = document.getElementById('edit-schedule-id');
        const nameInput = document.getElementById('edit-schedule-name');
        const exprInput = document.getElementById('edit-schedule-expr');
        const wsInput = document.getElementById('edit-schedule-workspace');
        const promptInput = document.getElementById('edit-schedule-prompt');
        const presetSelect = document.getElementById('schedule-preset-select');

        if (!pane) return;

        if (task) {
            title.textContent = 'Edit Automation';
            idInput.value = task.id || '';
            nameInput.value = task.name || '';
            exprInput.value = task.schedule || '';
            wsInput.value = task.workspace || '';
            promptInput.value = task.prompt || '';
        } else {
            title.textContent = 'New Scheduled Automation';
            idInput.value = '';
            nameInput.value = '';
            exprInput.value = '';
            wsInput.value = '';
            promptInput.value = '';
        }
        if (presetSelect) presetSelect.value = '';

        pane.style.display = 'block';
        isEditorOpen = true;
        nameInput.focus();
    }

    function closeEditor() {
        const pane = document.getElementById('schedules-editor-pane');
        if (pane) {
            pane.style.display = 'none';
        }
        isEditorOpen = false;
    }

    async function loadSchedules() {
        const container = document.getElementById('schedules-items-container');
        const badge = document.getElementById('schedules-count-badge');
        if (!container) return;

        // Also load starter automations section
        loadStarterAutomations();

        container.innerHTML = '<div class="schedules-empty-state"><div class="empty-icon">⏳</div><div class="empty-text">Loading automations...</div></div>';

        try {
            const res = await fetch('/api/oblivion/schedules', {
                headers: authHeaders()
            });

            if (!res.ok) {
                container.innerHTML = `<div class="schedules-empty-state"><div class="empty-icon">⚠️</div><div class="empty-text">Failed to load schedules (${res.status})</div></div>`;
                return;
            }

            const tasks = await res.json();
            currentSchedules = tasks;

            if (badge) {
                badge.textContent = `${tasks.length} automation${tasks.length === 1 ? '' : 's'}`;
            }

            if (!tasks || tasks.length === 0) {
                container.innerHTML = `
                    <div class="schedules-empty-state">
                        <div class="empty-icon">⏰</div>
                        <div class="empty-text">No scheduled automations yet</div>
                        <p style="color: var(--muted); font-size: 13px; margin-top: 6px;">Automate background jobs, morning briefs, repository checks, and recurring reminders.</p>
                        <button class="mem-btn mem-btn-primary" style="margin-top: 14px;" onclick="window.SchedulesModal.openEditor()">+ Create First Schedule</button>
                    </div>
                `;
                return;
            }

            renderSchedulesList(tasks, container);
        } catch (err) {
            console.error('Error loading schedules:', err);
            container.innerHTML = '<div class="schedules-empty-state"><div class="empty-icon">⚠️</div><div class="empty-text">Error connecting to scheduler</div></div>';
        }
    }

    function renderSchedulesList(tasks, container) {
        container.innerHTML = '';

        tasks.forEach(task => {
            const card = document.createElement('div');
            card.className = `schedule-card ${task.enabled ? 'active-task' : 'paused-task'}`;
            card.dataset.id = task.id;

            // Formatted next run
            let nextRunStr = 'None';
            if (task.next_run) {
                try {
                    const d = new Date(task.next_run);
                    nextRunStr = d.toLocaleString();
                } catch (e) {
                    nextRunStr = task.next_run;
                }
            }

            // Formatted last run
            let lastRunStr = 'Never run';
            let outcomeClass = 'outcome-never';
            let outcomeText = 'never';
            if (task.last_run) {
                try {
                    const d = new Date(task.last_run);
                    lastRunStr = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
                } catch (e) {
                    lastRunStr = task.last_run;
                }
                outcomeText = task.last_outcome || 'unknown';
                if (outcomeText === 'done') outcomeClass = 'outcome-done';
                else if (outcomeText === 'error') outcomeClass = 'outcome-err';
                else outcomeClass = 'outcome-other';
            }

            card.innerHTML = `
                <div class="schedule-card-header">
                    <div class="schedule-title-area">
                        <h4 class="schedule-card-name">${escapeHtml(task.name)}</h4>
                        <div class="schedule-badges">
                            <span class="schedule-pill-expr" title="Schedule pattern">${escapeHtml(task.schedule)}</span>
                            <span class="schedule-pill-mode">${task.is_recurring ? 'Recurring' : 'One-off'}</span>
                        </div>
                    </div>
                    <div class="schedule-toggle-area">
                        <label class="schedule-switch" title="${task.enabled ? 'Disable automation' : 'Enable automation'}">
                            <input type="checkbox" class="schedule-enable-toggle" ${task.enabled ? 'checked' : ''}>
                            <span class="schedule-slider"></span>
                        </label>
                    </div>
                </div>

                <div class="schedule-card-prompt">
                    ${escapeHtml(task.prompt)}
                </div>

                <div class="schedule-card-meta">
                    <div class="schedule-meta-item">
                        <span class="meta-label">Next Run:</span>
                        <span class="meta-value next-run-time">${escapeHtml(nextRunStr)}</span>
                    </div>
                    <div class="schedule-meta-item">
                        <span class="meta-label">Last Run:</span>
                        <span class="meta-badge ${outcomeClass}">${escapeHtml(outcomeText)}</span>
                        <span class="meta-sub">${escapeHtml(lastRunStr)}</span>
                    </div>
                    ${task.workspace ? `<div class="schedule-meta-item workspace-item" title="${escapeHtml(task.workspace)}"><span class="meta-label">Workspace:</span> <span class="meta-value">${escapeHtml(task.workspace)}</span></div>` : ''}
                </div>

                <div class="schedule-card-actions">
                    <button class="sched-btn sched-run-btn" title="Run automation right now">▶ Run Now</button>
                    <button class="sched-btn sched-edit-btn" title="Edit automation settings">✎ Edit</button>
                    <button class="sched-btn sched-del-btn" title="Delete automation">🗑</button>
                </div>
            `;

            // Bind toggle
            const toggle = card.querySelector('.schedule-enable-toggle');
            if (toggle) {
                toggle.addEventListener('change', async (e) => {
                    const isChecked = e.target.checked;
                    await handleToggleSchedule(task.id, isChecked);
                });
            }

            // Bind Run Now
            const runBtn = card.querySelector('.sched-run-btn');
            if (runBtn) {
                runBtn.addEventListener('click', async () => {
                    await handleRunNow(task.id, runBtn);
                });
            }

            // Bind Edit
            const editBtn = card.querySelector('.sched-edit-btn');
            if (editBtn) {
                editBtn.addEventListener('click', () => {
                    openEditor(task);
                });
            }

            // Bind Delete
            const delBtn = card.querySelector('.sched-del-btn');
            if (delBtn) {
                delBtn.addEventListener('click', async () => {
                    if (confirm(`Delete scheduled automation "${task.name}"?`)) {
                        await handleDeleteSchedule(task.id);
                    }
                });
            }

            container.appendChild(card);
        });
    }

    async function handleToggleSchedule(taskId, enabled) {
        try {
            const res = await fetch(`/api/oblivion/schedules/${taskId}`, {
                method: 'PATCH',
                headers: authHeaders(),
                body: JSON.stringify({ enabled })
            });
            if (res.ok) {
                loadSchedules();
            } else {
                alert('Failed to update schedule status.');
            }
        } catch (e) {
            console.error('Failed to toggle schedule:', e);
        }
    }

    async function handleRunNow(taskId, runBtn) {
        const originalText = runBtn.textContent;
        runBtn.disabled = true;
        runBtn.textContent = '⏳ Running...';

        try {
            const res = await fetch(`/api/oblivion/schedules/${taskId}/run`, {
                method: 'POST',
                headers: authHeaders(),
                body: JSON.stringify({})
            });

            if (res.ok) {
                const result = await res.json();
                runBtn.textContent = '✓ Done';
                setTimeout(() => {
                    loadSchedules();
                }, 1000);
            } else {
                const errData = await res.json().catch(() => ({}));
                alert(`Run failed: ${errData.error || res.statusText}`);
                runBtn.disabled = false;
                runBtn.textContent = originalText;
            }
        } catch (e) {
            console.error('Run failed:', e);
            alert(`Run failed: ${e.message}`);
            runBtn.disabled = false;
            runBtn.textContent = originalText;
        }
    }

    async function handleDeleteSchedule(taskId) {
        try {
            const res = await fetch(`/api/oblivion/schedules/${taskId}`, {
                method: 'DELETE',
                headers: authHeaders()
            });
            if (res.ok) {
                loadSchedules();
            } else {
                alert('Failed to delete schedule.');
            }
        } catch (e) {
            console.error('Delete error:', e);
        }
    }

    async function handleSaveSchedule() {
        const id = document.getElementById('edit-schedule-id').value.trim();
        const name = document.getElementById('edit-schedule-name').value.trim();
        const schedule = document.getElementById('edit-schedule-expr').value.trim();
        const workspace = document.getElementById('edit-schedule-workspace').value.trim();
        const prompt = document.getElementById('edit-schedule-prompt').value.trim();
        const saveBtn = document.getElementById('schedules-editor-save-btn');

        if (!name || !schedule || !prompt) {
            alert('Name, schedule expression, and prompt instructions are required.');
            return;
        }

        if (saveBtn) saveBtn.disabled = true;

        try {
            let res;
            if (id) {
                // Update
                res = await fetch(`/api/oblivion/schedules/${id}`, {
                    method: 'PATCH',
                    headers: authHeaders(),
                    body: JSON.stringify({
                        name,
                        schedule,
                        workspace: workspace || null,
                        prompt
                    })
                });
            } else {
                // Create
                res = await fetch('/api/oblivion/schedules', {
                    method: 'POST',
                    headers: authHeaders(),
                    body: JSON.stringify({
                        name,
                        schedule,
                        workspace: workspace || null,
                        prompt,
                        enabled: true
                    })
                });
            }

            if (res.ok) {
                closeEditor();
                loadSchedules();
            } else {
                const errData = await res.json().catch(() => ({}));
                alert(`Failed to save automation: ${errData.error || res.statusText}`);
            }
        } catch (e) {
            console.error('Save error:', e);
            alert(`Error saving automation: ${e.message}`);
        } finally {
            if (saveBtn) saveBtn.disabled = false;
        }
    }

    async function loadStarterAutomations() {
        const grid = document.getElementById('schedules-starters-grid');
        if (!grid) return;

        try {
            const res = await fetch('/api/oblivion/schedules/starters', {
                headers: authHeaders()
            });
            if (!res.ok) return;

            const starters = await res.json();
            grid.innerHTML = '';

            starters.forEach(s => {
                const card = document.createElement('div');
                card.className = `starter-card ${s.installed ? 'installed' : ''}`;
                card.dataset.id = s.id;

                const actionBtn = s.installed
                    ? `<button class="mem-btn btn-sm btn-starter-active" disabled title="Currently active in schedules">✓ Active</button>`
                    : `<button class="mem-btn btn-sm btn-starter-install" data-id="${escapeHtml(s.id)}" title="Add ${escapeHtml(s.title)}">+ Add Starter</button>`;

                card.innerHTML = `
                    <div class="starter-card-header">
                        <span class="starter-icon">${s.icon || '⚡'}</span>
                        <div class="starter-title-group">
                            <span class="starter-title">${escapeHtml(s.title)}</span>
                            <span class="starter-sched-badge">${escapeHtml(s.schedule)}</span>
                        </div>
                    </div>
                    <div class="starter-desc">${escapeHtml(s.description)}</div>
                    <div class="starter-card-actions">
                        ${actionBtn}
                    </div>
                `;

                grid.appendChild(card);
            });

            // Attach install clicks
            grid.querySelectorAll('.btn-starter-install').forEach(btn => {
                btn.addEventListener('click', async (e) => {
                    e.stopPropagation();
                    const id = btn.getAttribute('data-id');
                    await installStarterAutomation(id, btn);
                });
            });
        } catch (err) {
            console.error('Failed to load starter automations:', err);
        }
    }

    async function installStarterAutomation(id, btn = null) {
        if (btn) {
            btn.disabled = true;
            btn.textContent = '⏳ Adding...';
        }
        try {
            const res = await fetch(`/api/oblivion/schedules/starters/${encodeURIComponent(id)}/install`, {
                method: 'POST',
                headers: authHeaders()
            });
            if (!res.ok) {
                const err = await res.json().catch(() => ({}));
                throw new Error(err.error || `HTTP ${res.status}`);
            }
            const task = await res.json();
            if (window.JarvisApprovals && window.JarvisApprovals.showToast) {
                window.JarvisApprovals.showToast({
                    id: 'starter-' + Date.now(),
                    title: 'Starter Automation Added',
                    body: `"${task.name}" is now active with schedule "${task.schedule}".`,
                    kind: 'info'
                });
            }
            await loadStarterAutomations();
            await loadSchedules();
        } catch (err) {
            console.error('Failed to install starter automation:', err);
            alert(`Failed to add starter automation: ${err.message}`);
            if (btn) {
                btn.disabled = false;
                btn.textContent = '+ Add Starter';
            }
        }
    }

    function escapeHtml(str) {
        if (!str) return '';
        return String(str)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#039;');
    }

    // Expose public API
    window.SchedulesModal = {
        open: openSchedulesModal,
        close: closeSchedulesModal,
        toggle: toggleSchedulesModal,
        openEditor: () => openEditor(),
        reload: loadSchedules
    };

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', initSchedulesWindow);
    } else {
        initSchedulesWindow();
    }
})();
