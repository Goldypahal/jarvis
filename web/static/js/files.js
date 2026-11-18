// Jarvis Workspace Files & Collections Manager (B7)
// Manages the #filesModal window, displaying real files created and modified in the workspace.

(function () {
    let filesData = [];
    let activeFilter = 'all';

    function init() {
        const modal = document.getElementById('filesModal');
        const closeBtn = document.getElementById('close-files-modal-btn');
        const searchInput = document.getElementById('files-search-input');
        const filterPills = document.querySelectorAll('.files-pill');

        if (closeBtn && modal) {
            closeBtn.addEventListener('click', () => modal.classList.remove('active'));
        }

        if (modal) {
            modal.addEventListener('click', (e) => {
                if (e.target === modal) modal.classList.remove('active');
            });
        }

        if (searchInput) {
            searchInput.addEventListener('input', () => renderFiles());
        }

        filterPills.forEach(pill => {
            pill.addEventListener('click', () => {
                filterPills.forEach(p => p.classList.remove('active'));
                pill.classList.add('active');
                activeFilter = pill.dataset.filter || 'all';
                renderFiles();
            });
        });
    }

    async function loadFiles() {
        try {
            const token = window.Oblivion?.token || new URLSearchParams(window.location.search).get('t') || '';
            const headers = token ? { 'x-oblivion-token': token } : {};
            const res = await fetch('/api/oblivion/dashboard', { headers });
            if (res.ok) {
                const data = await res.json();
                filesData = data.recent_files || [];
                renderFiles();
            } else {
                renderError('Could not load workspace files.');
            }
        } catch (err) {
            console.error('Error loading files:', err);
            renderError('Failed to fetch workspace files.');
        }
    }

    function formatBytes(bytes) {
        if (!bytes || bytes === 0) return '0 B';
        const k = 1024;
        const sizes = ['B', 'KB', 'MB', 'GB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
    }

    function formatDate(secs) {
        if (!secs) return '';
        const d = new Date(secs * 1000);
        return d.toLocaleDateString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
    }

    function getCategoryIcon(cat) {
        switch (cat) {
            case 'paper': return '📄';
            case 'picture': return '🎨';
            case 'document': return '📁';
            case 'notebook': return '📓';
            case 'case-study': return '📂';
            case 'code': return '💻';
            default: return '📄';
        }
    }

    function renderFiles() {
        const container = document.getElementById('files-items-container');
        const badge = document.getElementById('files-modal-count-badge');
        const searchInput = document.getElementById('files-search-input');
        const query = (searchInput?.value || '').toLowerCase().trim();

        if (!container) return;

        let filtered = filesData;
        if (activeFilter && activeFilter !== 'all') {
            filtered = filtered.filter(f => f.category === activeFilter);
        }

        if (query) {
            filtered = filtered.filter(f =>
                f.name.toLowerCase().includes(query) || f.path.toLowerCase().includes(query)
            );
        }

        if (badge) {
            badge.textContent = `${filtered.length} of ${filesData.length} files`;
        }

        if (filtered.length === 0) {
            container.innerHTML = `
                <div class="files-empty-state">
                    <div class="empty-icon">📁</div>
                    <div class="empty-text">${filesData.length === 0 ? 'No files created in workspace yet.' : 'No files match your search.'}</div>
                </div>
            `;
            return;
        }

        container.innerHTML = filtered.map(f => `
            <div class="file-card-item" data-path="${escapeHtml(f.path)}">
                <div class="file-card-icon">${getCategoryIcon(f.category)}</div>
                <div class="file-card-info">
                    <div class="file-card-name" title="${escapeHtml(f.name)}">${escapeHtml(f.name)}</div>
                    <div class="file-card-path" title="${escapeHtml(f.path)}">${escapeHtml(f.path)}</div>
                    <div class="file-card-meta">
                        <span class="file-card-size">${formatBytes(f.size)}</span>
                        <span class="file-card-dot">·</span>
                        <span class="file-card-date">${formatDate(f.modified)}</span>
                        <span class="file-card-cat-badge">${escapeHtml(f.category)}</span>
                    </div>
                </div>
                <div class="file-card-actions">
                    <button class="file-action-btn view-file-btn" title="Inspect with Oblivion" data-path="${escapeHtml(f.path)}">Inspect</button>
                </div>
            </div>
        `).join('');

        // Attach action handlers
        container.querySelectorAll('.view-file-btn').forEach(btn => {
            btn.addEventListener('click', (e) => {
                e.stopPropagation();
                const path = btn.dataset.path;
                const modal = document.getElementById('filesModal');
                if (modal) modal.classList.remove('active');
                if (window.openSplitModalWithContext) {
                    window.openSplitModalWithContext(`Read and inspect workspace file: ${path}`);
                }
            });
        });
    }

    function renderError(msg) {
        const container = document.getElementById('files-items-container');
        if (container) {
            container.innerHTML = `
                <div class="files-empty-state">
                    <div class="empty-icon">⚠️</div>
                    <div class="empty-text">${escapeHtml(msg)}</div>
                </div>
            `;
        }
    }

    function open(filter) {
        const modal = document.getElementById('filesModal');
        if (modal) {
            modal.classList.add('active');
            if (filter) {
                activeFilter = filter;
                document.querySelectorAll('.files-pill').forEach(pill => {
                    pill.classList.toggle('active', pill.dataset.filter === filter);
                });
            }
            loadFiles();
        }
    }

    function openWithFilter(filter) {
        open(filter);
    }

    function escapeHtml(str) {
        return (str || '').replace(/[&<>"']/g, function (m) {
            return {
                '&': '&amp;',
                '<': '&lt;',
                '>': '&gt;',
                '"': '&quot;',
                "'": '&#039;'
            }[m];
        });
    }

    document.addEventListener('DOMContentLoaded', init);

    window.FilesManager = {
        open,
        openWithFilter,
        loadFiles
    };
})();
