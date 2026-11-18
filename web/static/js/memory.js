// Jarvis Desktop Memory Window Controller
// Interacts with /api/oblivion/memory endpoints for listing, searching, editing, deleting, and exporting memories.

(function () {
    let currentMemories = [];
    let isEditorOpen = false;
    let searchDebounceTimeout = null;

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

    function initMemoryWindow() {
        const modal = document.getElementById('memoryModal');
        const openDockBtn = document.getElementById('open-memory-dock-btn');
        const closeBtn = document.getElementById('close-memory-modal-btn');
        const searchInput = document.getElementById('memory-search-input');
        const typeFilter = document.getElementById('memory-type-filter');
        const newBtn = document.getElementById('memory-new-btn');
        const forgetTopicBtn = document.getElementById('memory-forget-topic-btn');
        const exportJsonBtn = document.getElementById('memory-export-json-btn');
        const exportMdBtn = document.getElementById('memory-export-md-btn');
        const editorCloseBtn = document.getElementById('memory-editor-close-btn');
        const editorCancelBtn = document.getElementById('memory-editor-cancel-btn');
        const editorForm = document.getElementById('memory-editor-form');

        // Toggle / Open
        if (openDockBtn) {
            openDockBtn.addEventListener('click', () => {
                toggleMemoryModal();
            });
        }

        if (closeBtn && modal) {
            closeBtn.addEventListener('click', () => {
                closeMemoryModal();
            });
        }

        if (modal) {
            modal.addEventListener('click', (e) => {
                if (e.target === modal) {
                    closeMemoryModal();
                }
            });
        }

        // Global keyboard shortcut: Ctrl+M / Cmd+M
        document.addEventListener('keydown', (e) => {
            if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'm') {
                e.preventDefault();
                toggleMemoryModal();
            }
        });

        // Search and Filtering
        if (searchInput) {
            searchInput.addEventListener('input', () => {
                clearTimeout(searchDebounceTimeout);
                searchDebounceTimeout = setTimeout(() => {
                    loadMemories();
                }, 250);
            });
        }

        if (typeFilter) {
            typeFilter.addEventListener('change', () => {
                loadMemories();
            });
        }

        // New Memory Button
        if (newBtn) {
            newBtn.addEventListener('click', () => {
                openEditor(null);
            });
        }

        // Editor Close / Cancel
        if (editorCloseBtn) {
            editorCloseBtn.addEventListener('click', closeEditor);
        }
        if (editorCancelBtn) {
            editorCancelBtn.addEventListener('click', closeEditor);
        }

        // Editor Form Submit (Create or Update)
        if (editorForm) {
            editorForm.addEventListener('submit', async (e) => {
                e.preventDefault();
                await saveMemoryForm();
            });
        }

        // Forget Topic Button
        if (forgetTopicBtn) {
            forgetTopicBtn.addEventListener('click', async () => {
                const topic = prompt('Forget everything about topic / query:\n(This will delete matching memories)');
                if (topic && topic.trim()) {
                    await forgetTopic(topic.trim());
                }
            });
        }

        // Export Buttons
        if (exportJsonBtn) {
            exportJsonBtn.addEventListener('click', () => {
                exportMemories('json');
            });
        }

        if (exportMdBtn) {
            exportMdBtn.addEventListener('click', () => {
                exportMemories('markdown');
            });
        }
    }

    function toggleMemoryModal() {
        const modal = document.getElementById('memoryModal');
        if (!modal) return;
        if (modal.classList.contains('active')) {
            closeMemoryModal();
        } else {
            openMemoryModal();
        }
    }

    function openMemoryModal() {
        const modal = document.getElementById('memoryModal');
        if (modal) {
            modal.classList.add('active');
            loadMemories();
            const searchInput = document.getElementById('memory-search-input');
            if (searchInput) {
                setTimeout(() => searchInput.focus(), 150);
            }
        }
    }

    function closeMemoryModal() {
        const modal = document.getElementById('memoryModal');
        if (modal) {
            modal.classList.remove('active');
            closeEditor();
        }
    }

    async function loadMemories() {
        const container = document.getElementById('memory-items-container');
        const countBadge = document.getElementById('memory-count-badge');
        const searchInput = document.getElementById('memory-search-input');
        const typeFilter = document.getElementById('memory-type-filter');

        const query = searchInput ? searchInput.value.trim() : '';
        const type = typeFilter ? typeFilter.value.trim() : '';

        const params = new URLSearchParams();
        if (query) params.set('q', query);
        if (type) params.set('type', type);
        params.set('limit', '100');

        try {
            const res = await fetch(`/api/oblivion/memory?${params.toString()}`, {
                headers: authHeaders()
            });
            if (!res.ok) {
                throw new Error(`Server returned ${res.status}`);
            }
            const data = await res.json();
            currentMemories = data;
            if (countBadge) {
                countBadge.textContent = `${data.length} ${data.length === 1 ? 'memory' : 'memories'}`;
            }
            renderMemories(data, container);
        } catch (err) {
            console.error('Failed to load memories:', err);
            if (container) {
                container.innerHTML = `<div class="memory-empty">Could not load memories: ${escapeHtml(err.message)}</div>`;
            }
        }
    }

    function renderMemories(memories, container) {
        if (!container) return;
        if (!memories || memories.length === 0) {
            container.innerHTML = '<div class="memory-empty">No memories found. Add one with "+ Add Memory" or let Oblivion propose memories after sessions.</div>';
            return;
        }

        container.innerHTML = '';
        memories.forEach(m => {
            const card = document.createElement('div');
            card.className = 'memory-card';
            card.dataset.id = m.id;

            const stars = '★'.repeat(m.importance || 3) + '☆'.repeat(5 - (m.importance || 3));
            const projHtml = m.project ? `<span class="mem-proj-tag" title="Project scope">📁 ${escapeHtml(m.project)}</span>` : '';
            const relativeTime = formatRelativeTime(m.last_used || m.created);

            card.innerHTML = `
                <div class="mem-card-head">
                    <div class="mem-card-tags">
                        <span class="mem-badge mem-badge-${m.mem_type}">${escapeHtml(m.mem_type)}</span>
                        ${projHtml}
                        <span class="mem-stars" title="Importance: ${m.importance}/5">${stars}</span>
                    </div>
                    <span class="mem-time">${relativeTime}</span>
                </div>
                <div class="mem-card-content">${escapeHtml(m.text)}</div>
                <div class="mem-card-actions">
                    <button class="mem-btn-ghost mem-edit-btn" data-id="${m.id}" title="Edit this memory">Edit</button>
                    <button class="mem-btn-ghost mem-btn-danger mem-del-btn" data-id="${m.id}" title="Delete this memory">Delete</button>
                </div>
            `;

            // Action listeners
            const editBtn = card.querySelector('.mem-edit-btn');
            const delBtn = card.querySelector('.mem-del-btn');

            if (editBtn) {
                editBtn.addEventListener('click', () => {
                    openEditor(m);
                });
            }

            if (delBtn) {
                delBtn.addEventListener('click', async () => {
                    if (confirm(`Delete memory #${m.id}?\n"${m.text.slice(0, 70)}..."`)) {
                        await deleteMemory(m.id);
                    }
                });
            }

            container.appendChild(card);
        });
    }

    function openEditor(memory) {
        const pane = document.getElementById('memory-editor-pane');
        const title = document.getElementById('memory-editor-title');
        const idInput = document.getElementById('edit-memory-id');
        const typeSelect = document.getElementById('edit-memory-type');
        const impInput = document.getElementById('edit-memory-importance');
        const projectInput = document.getElementById('edit-memory-project');
        const textInput = document.getElementById('edit-memory-text');

        if (!pane) return;

        if (memory) {
            title.textContent = `Edit Memory #${memory.id}`;
            idInput.value = memory.id;
            typeSelect.value = memory.mem_type || 'fact';
            impInput.value = memory.importance || 3;
            projectInput.value = memory.project || '';
            textInput.value = memory.text || '';
        } else {
            title.textContent = 'New Memory';
            idInput.value = '';
            typeSelect.value = 'user_pref';
            impInput.value = 3;
            projectInput.value = '';
            textInput.value = '';
        }

        pane.style.display = 'block';
        isEditorOpen = true;
        textInput.focus();
    }

    function closeEditor() {
        const pane = document.getElementById('memory-editor-pane');
        if (pane) {
            pane.style.display = 'none';
        }
        isEditorOpen = false;
    }

    async function saveMemoryForm() {
        const id = document.getElementById('edit-memory-id').value;
        const mem_type = document.getElementById('edit-memory-type').value;
        const importance = parseInt(document.getElementById('edit-memory-importance').value, 10) || 3;
        const project = document.getElementById('edit-memory-project').value.trim();
        const text = document.getElementById('edit-memory-text').value.trim();

        if (!text) {
            alert('Memory content cannot be empty.');
            return;
        }

        try {
            if (id) {
                // Update existing
                const res = await fetch(`/api/oblivion/memory/${id}`, {
                    method: 'PUT',
                    headers: authHeaders(),
                    body: JSON.stringify({
                        text,
                        type: mem_type,
                        importance
                    })
                });
                if (!res.ok) throw new Error(`Update failed (${res.status})`);
            } else {
                // Create new
                const res = await fetch('/api/oblivion/memory', {
                    method: 'POST',
                    headers: authHeaders(),
                    body: JSON.stringify({
                        text,
                        type: mem_type,
                        importance,
                        project: project || null
                    })
                });
                if (!res.ok) throw new Error(`Create failed (${res.status})`);
            }
            closeEditor();
            await loadMemories();
        } catch (err) {
            console.error('Failed to save memory:', err);
            alert(`Error saving memory: ${err.message}`);
        }
    }

    async function deleteMemory(id) {
        try {
            const res = await fetch(`/api/oblivion/memory/${id}`, {
                method: 'DELETE',
                headers: authHeaders()
            });
            if (!res.ok) throw new Error(`Delete failed (${res.status})`);
            await loadMemories();
        } catch (err) {
            console.error('Failed to delete memory:', err);
            alert(`Error deleting memory: ${err.message}`);
        }
    }

    async function forgetTopic(query) {
        try {
            const res = await fetch('/api/oblivion/memory/forget', {
                method: 'POST',
                headers: authHeaders(),
                body: JSON.stringify({ query })
            });
            if (!res.ok) throw new Error(`Forget topic failed (${res.status})`);
            const data = await res.json();
            alert(`Deleted ${data.deleted_count} memories matching '${data.query}'.`);
            await loadMemories();
        } catch (err) {
            console.error('Failed to forget topic:', err);
            alert(`Error forgetting topic: ${err.message}`);
        }
    }

    function exportMemories(format) {
        const token = getToken();
        const url = `/api/oblivion/memory/export?format=${encodeURIComponent(format)}&t=${encodeURIComponent(token)}`;
        window.open(url, '_blank');
    }

    function formatRelativeTime(dateStr) {
        if (!dateStr) return '';
        try {
            const d = new Date(dateStr);
            if (isNaN(d.getTime())) return dateStr;
            const diffMs = Date.now() - d.getTime();
            const diffMin = Math.floor(diffMs / 60000);
            if (diffMin < 1) return 'just now';
            if (diffMin < 60) return `${diffMin}m ago`;
            const diffHours = Math.floor(diffMin / 60);
            if (diffHours < 24) return `${diffHours}h ago`;
            const diffDays = Math.floor(diffHours / 24);
            if (diffDays < 30) return `${diffDays}d ago`;
            return d.toLocaleDateString();
        } catch (_) {
            return dateStr;
        }
    }

    function escapeHtml(str) {
        if (!str) return '';
        return String(str).replace(/[&<>"']/g, function(m) {
            return {
                '&': '&amp;',
                '<': '&lt;',
                '>': '&gt;',
                '"': '&quot;',
                "'": '&#039;'
            }[m];
        });
    }

    document.addEventListener('DOMContentLoaded', initMemoryWindow);

    window.JarvisMemory = {
        open: openMemoryModal,
        close: closeMemoryModal,
        toggle: toggleMemoryModal,
        reload: loadMemories
    };
})();
