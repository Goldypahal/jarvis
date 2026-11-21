/**
 * Connectors & MCP Integrations Management (Month 6)
 * Handles listing MCP servers, toggling enablement in oblivion.toml,
 * saving credentials encrypted in Windows Credential Manager via /api/oblivion/connectors,
 * and testing live server connectivity.
 */
(() => {
    'use strict';

    let connectorsList = [];
    let isLoading = false;

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

    function escapeHtml(str) {
        if (!str) return '';
        return String(str)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }

    function showToast(message, type = 'info', title = 'MCP Connectors') {
        const container = document.getElementById('toast-container');
        if (!container) return;

        let icon = '🔌';
        if (type === 'success') icon = '✅';
        else if (type === 'error') icon = '❌';
        else if (type === 'warning') icon = '⚠️';

        const toast = document.createElement('div');
        toast.className = 'toast-card';
        toast.innerHTML = `
            <div class="toast-icon">${icon}</div>
            <div class="toast-body">
                <div class="toast-title">${escapeHtml(title)}</div>
                <div class="toast-message">${escapeHtml(message)}</div>
            </div>
            <div class="toast-actions">
                <button class="toast-action-btn" title="Dismiss">✕</button>
            </div>
        `;

        const dismissBtn = toast.querySelector('.toast-action-btn');
        if (dismissBtn) {
            dismissBtn.addEventListener('click', (e) => {
                e.stopPropagation();
                toast.classList.add('dismissed');
                setTimeout(() => toast.remove(), 250);
            });
        }

        container.appendChild(toast);

        // Auto dismiss after 5 seconds
        setTimeout(() => {
            if (toast.parentElement) {
                toast.classList.add('dismissed');
                setTimeout(() => toast.remove(), 250);
            }
        }, 5000);
    }

    async function fetchConnectors() {
        if (isLoading) return;
        isLoading = true;

        const grid = document.getElementById('connectors-cards-grid');
        const badge = document.getElementById('connectors-count-badge');
        if (grid && connectorsList.length === 0) {
            grid.innerHTML = `
                <div class="connectors-empty-state">
                    <div class="empty-icon">🔌</div>
                    <div class="empty-text">Loading connectors...</div>
                </div>
            `;
        }

        try {
            const res = await fetch('/api/oblivion/connectors', {
                headers: { 'X-Jarvis-Token': getToken() }
            });
            if (!res.ok) {
                throw new Error(`HTTP ${res.status}: ${res.statusText}`);
            }
            const data = await res.json();
            connectorsList = Array.isArray(data) ? data : (data.connectors || []);
            renderConnectors();
        } catch (err) {
            console.error('Failed to load connectors:', err);
            if (grid) {
                grid.innerHTML = `
                    <div class="connectors-empty-state">
                        <div class="empty-icon">⚠️</div>
                        <div class="empty-text">Failed to load connectors</div>
                        <div class="empty-sub">${escapeHtml(err.message)}</div>
                    </div>
                `;
            }
            if (badge) badge.textContent = 'Error';
        } finally {
            isLoading = false;
        }
    }

    function renderConnectors() {
        const grid = document.getElementById('connectors-cards-grid');
        const badge = document.getElementById('connectors-count-badge');
        if (!grid) return;

        if (connectorsList.length === 0) {
            grid.innerHTML = `
                <div class="connectors-empty-state">
                    <div class="empty-icon">🔌</div>
                    <div class="empty-text">No connectors configured</div>
                    <div class="empty-sub">Add MCP servers to oblivion.toml or configure preset connectors.</div>
                </div>
            `;
            if (badge) badge.textContent = '0 configured';
            return;
        }

        const enabledCount = connectorsList.filter(c => c.enabled).length;
        if (badge) {
            badge.textContent = `${enabledCount} of ${connectorsList.length} enabled`;
        }

        grid.innerHTML = '';
        connectorsList.forEach(connector => {
            const card = document.createElement('div');
            card.className = `connector-card ${connector.enabled ? 'enabled' : 'disabled'}`;
            card.setAttribute('data-name', connector.name);

            // Badges
            let keyBadge = '';
            if (connector.key_configured) {
                keyBadge = '<span class="badge badge-key-ok">🔒 Credential Stored</span>';
            } else if (connector.token_env) {
                keyBadge = '<span class="badge badge-key-needed">⚠️ Key Needed</span>';
            }

            const commandHtml = connector.command 
                ? `<span class="meta-label">Cmd:</span> <code class="meta-code">${escapeHtml(connector.command)}</code>` 
                : '';
            const envHtml = connector.token_env 
                ? `<span class="meta-label">Env:</span> <code class="meta-code">${escapeHtml(connector.token_env)}</code>` 
                : '';

            card.innerHTML = `
                <div class="connector-card-header">
                    <div class="connector-identity">
                        <div class="connector-icon">${connector.icon || '🔌'}</div>
                        <div class="connector-titles">
                            <div class="connector-name-row">
                                <span class="connector-title-text">${escapeHtml(connector.title)}</span>
                                <span class="badge badge-transport">${escapeHtml(connector.transport)}</span>
                                ${keyBadge}
                            </div>
                            <div class="connector-desc">${escapeHtml(connector.description)}</div>
                        </div>
                    </div>
                    <div class="connector-toggle-wrapper">
                        <label class="switch-toggle" title="${connector.enabled ? 'Disable' : 'Enable'} ${escapeHtml(connector.title)}">
                            <input type="checkbox" class="connector-toggle-input" data-name="${escapeHtml(connector.name)}" ${connector.enabled ? 'checked' : ''}>
                            <span class="switch-slider"></span>
                        </label>
                    </div>
                </div>

                <div class="connector-card-details">
                    <div class="connector-meta-row">
                        <span class="meta-label">ID:</span> <code class="meta-code">${escapeHtml(connector.name)}</code>
                        ${commandHtml}
                        ${envHtml}
                    </div>
                    ${connector.security_note ? `
                        <div class="connector-security-note">
                            <span class="sec-icon">🛡️</span>
                            <span class="sec-text">${escapeHtml(connector.security_note)}</span>
                        </div>
                    ` : ''}
                </div>

                <div class="connector-key-input-row" id="key-row-${escapeHtml(connector.name)}" style="display: none;">
                    <input type="password" class="connector-key-field" id="key-field-${escapeHtml(connector.name)}" placeholder="Paste API token or key here..." autocomplete="new-password">
                    <button class="mem-btn btn-save-key" data-name="${escapeHtml(connector.name)}">Save to Credential Mgr</button>
                    <button class="mem-btn btn-cancel-key" data-name="${escapeHtml(connector.name)}">Cancel</button>
                </div>

                <div class="connector-card-actions">
                    <div class="actions-left">
                        ${connector.token_env ? `
                            <button class="mem-btn btn-toggle-key-input" data-name="${escapeHtml(connector.name)}">
                                🔑 ${connector.key_configured ? 'Change Key' : 'Set Key'}
                            </button>
                            ${connector.key_configured ? `
                                <button class="mem-btn btn-clear-key text-danger" data-name="${escapeHtml(connector.name)}" title="Remove credential from Windows Credential Manager">
                                    🗑️ Clear Key
                                </button>
                            ` : ''}
                        ` : ''}
                    </div>
                    <div class="actions-right">
                        <button class="mem-btn btn-test-conn" data-name="${escapeHtml(connector.name)}" ${!connector.enabled ? 'disabled' : ''}>
                            ⚡ Test Connection
                        </button>
                    </div>
                </div>
                <div class="connector-test-feedback" id="test-feedback-${escapeHtml(connector.name)}" style="display: none;"></div>
            `;

            grid.appendChild(card);
        });

        attachCardEventListeners();
    }

    function attachCardEventListeners() {
        // Toggle Switch
        document.querySelectorAll('.connector-toggle-input').forEach(input => {
            input.addEventListener('change', async (e) => {
                const name = e.target.getAttribute('data-name');
                const enabled = e.target.checked;
                await toggleConnector(name, enabled);
            });
        });

        // Key Input Form Toggle
        document.querySelectorAll('.btn-toggle-key-input').forEach(btn => {
            btn.addEventListener('click', (e) => {
                const name = e.currentTarget.getAttribute('data-name');
                const row = document.getElementById(`key-row-${name}`);
                if (row) {
                    const isVisible = row.style.display !== 'none';
                    row.style.display = isVisible ? 'none' : 'flex';
                    if (!isVisible) {
                        const field = document.getElementById(`key-field-${name}`);
                        if (field) field.focus();
                    }
                }
            });
        });

        // Cancel Key Form
        document.querySelectorAll('.btn-cancel-key').forEach(btn => {
            btn.addEventListener('click', (e) => {
                const name = e.currentTarget.getAttribute('data-name');
                const row = document.getElementById(`key-row-${name}`);
                if (row) row.style.display = 'none';
            });
        });

        // Save Key
        document.querySelectorAll('.btn-save-key').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                const name = e.currentTarget.getAttribute('data-name');
                const field = document.getElementById(`key-field-${name}`);
                if (!field) return;
                const key = field.value.trim();
                if (!key) {
                    showToast('Please enter a key before saving.', 'warning');
                    return;
                }
                await saveConnectorKey(name, key);
            });
        });

        // Clear Key
        document.querySelectorAll('.btn-clear-key').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                const name = e.currentTarget.getAttribute('data-name');
                await deleteConnectorKey(name);
            });
        });

        // Test Connection
        document.querySelectorAll('.btn-test-conn').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                const name = e.currentTarget.getAttribute('data-name');
                await testConnector(name, e.currentTarget);
            });
        });
    }

    async function toggleConnector(name, enabled) {
        try {
            const res = await fetch(`/api/oblivion/connectors/${encodeURIComponent(name)}/toggle`, {
                method: 'POST',
                headers: authHeaders(),
                body: JSON.stringify({ enabled })
            });
            if (!res.ok) {
                const err = await res.json().catch(() => ({}));
                throw new Error(err.error || `HTTP ${res.status}`);
            }
            showToast(`${name} is now ${enabled ? 'enabled' : 'disabled'}.`, 'success');
            await fetchConnectors();
        } catch (err) {
            console.error('Failed to toggle connector:', err);
            showToast(`Failed to toggle ${name}: ${err.message}`, 'error');
            await fetchConnectors();
        }
    }

    async function saveConnectorKey(name, key) {
        try {
            const res = await fetch(`/api/oblivion/connectors/${encodeURIComponent(name)}/key`, {
                method: 'POST',
                headers: authHeaders(),
                body: JSON.stringify({ key })
            });
            if (!res.ok) {
                const err = await res.json().catch(() => ({}));
                throw new Error(err.error || `HTTP ${res.status}`);
            }
            showToast(`Key for ${name} saved securely in Windows Credential Manager.`, 'success');
            await fetchConnectors();
        } catch (err) {
            console.error('Failed to save key:', err);
            showToast(`Failed to save key for ${name}: ${err.message}`, 'error');
        }
    }

    async function deleteConnectorKey(name) {
        try {
            const res = await fetch(`/api/oblivion/connectors/${encodeURIComponent(name)}/key`, {
                method: 'DELETE',
                headers: { 'X-Jarvis-Token': getToken() }
            });
            if (!res.ok) {
                const err = await res.json().catch(() => ({}));
                throw new Error(err.error || `HTTP ${res.status}`);
            }
            showToast(`Key for ${name} removed from Windows Credential Manager.`, 'info');
            await fetchConnectors();
        } catch (err) {
            console.error('Failed to delete key:', err);
            showToast(`Failed to clear key: ${err.message}`, 'error');
        }
    }

    async function testConnector(name, btn) {
        const feedback = document.getElementById(`test-feedback-${name}`);
        const origText = btn.innerHTML;
        btn.disabled = true;
        btn.innerHTML = '⏳ Testing...';

        if (feedback) {
            feedback.style.display = 'none';
            feedback.className = 'connector-test-result';
        }

        try {
            const res = await fetch(`/api/oblivion/connectors/${encodeURIComponent(name)}/test`, {
                method: 'POST',
                headers: authHeaders()
            });
            const data = await res.json().catch(() => ({}));

            if (res.ok && data.status === 'ok') {
                const msg = data.message || `Connector ${name} passed health check!`;
                showToast(`✓ ${msg}`, 'success', `Test: ${name}`);
                if (feedback) {
                    feedback.className = 'connector-test-result success';
                    feedback.textContent = `✓ ${msg}`;
                    feedback.style.display = 'flex';
                }
            } else {
                const msg = data.error || `Connector ${name} test failed.`;
                showToast(`✕ ${msg}`, 'error', `Test: ${name}`);
                if (feedback) {
                    feedback.className = 'connector-test-result error';
                    feedback.textContent = `✕ ${msg}`;
                    feedback.style.display = 'flex';
                }
            }
        } catch (err) {
            console.error('Failed to test connector:', err);
            showToast(`✕ Failed to test ${name}: ${err.message}`, 'error', `Test: ${name}`);
            if (feedback) {
                feedback.className = 'connector-test-result error';
                feedback.textContent = `✕ ${err.message}`;
                feedback.style.display = 'flex';
            }
        } finally {
            btn.disabled = false;
            btn.innerHTML = origText;
        }
    }

    function openModal() {
        const modal = document.getElementById('connectorsModal');
        if (!modal) return;
        modal.classList.add('active');
        fetchConnectors();
    }

    function closeModal() {
        const modal = document.getElementById('connectorsModal');
        if (!modal) return;
        modal.classList.remove('active');
    }

    function initConnectors() {
        const dockBtn = document.getElementById('open-connectors-dock-btn');
        const closeBtn = document.getElementById('close-connectors-modal-btn');
        const refreshBtn = document.getElementById('connectors-refresh-btn');
        const modal = document.getElementById('connectorsModal');

        if (dockBtn) {
            dockBtn.addEventListener('click', (e) => {
                e.preventDefault();
                openModal();
            });
        }

        if (closeBtn) {
            closeBtn.addEventListener('click', (e) => {
                e.preventDefault();
                closeModal();
            });
        }

        if (refreshBtn) {
            refreshBtn.addEventListener('click', (e) => {
                e.preventDefault();
                fetchConnectors();
            });
        }

        if (modal) {
            modal.addEventListener('click', (e) => {
                if (e.target === modal) {
                    closeModal();
                }
            });
        }

        document.addEventListener('keydown', (e) => {
            if (e.key === 'Escape' && modal && modal.classList.contains('active')) {
                closeModal();
            }
        });
    }

    window.JarvisConnectors = {
        open: openModal,
        close: closeModal,
        refresh: fetchConnectors,
        toggle: toggleConnector,
        saveKey: saveConnectorKey,
        deleteKey: deleteConnectorKey,
        test: testConnector
    };

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', initConnectors);
    } else {
        initConnectors();
    }
})();
