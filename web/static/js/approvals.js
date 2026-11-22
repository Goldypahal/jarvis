// Jarvis Desktop Approvals Queue & Notifications Window Controller
// Manages pending approvals from unattended tasks, desktop toasts, and notification history.

(function () {
    let pendingApprovals = [];
    let notifications = [];
    let seenNotificationIds = new Set();
    let isInitialized = false;
    let currentTab = 'approvals'; // 'approvals' | 'notifications'
    let pollTimer = null;

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
        const div = document.createElement('div');
        div.textContent = str;
        return div.innerHTML;
    }

    function formatTime(isoString) {
        if (!isoString) return '';
        try {
            const date = new Date(isoString);
            return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
        } catch {
            return isoString;
        }
    }

    function initApprovals() {
        const modal = document.getElementById('approvalsModal');
        const menubarBtn = document.getElementById('menubar-approvals-btn');
        const dockBtn = document.getElementById('open-approvals-dock-btn');
        const closeBtn = document.getElementById('close-approvals-modal-btn');
        const tabApprovals = document.getElementById('tab-btn-approvals');
        const tabNotifications = document.getElementById('tab-btn-notifications');
        const refreshBtn = document.getElementById('approvals-refresh-btn');
        const markAllBtn = document.getElementById('notifications-mark-all-btn');

        if (menubarBtn) {
            menubarBtn.addEventListener('click', () => toggleApprovalsModal());
        }

        if (dockBtn) {
            dockBtn.addEventListener('click', () => toggleApprovalsModal());
        }

        if (closeBtn && modal) {
            closeBtn.addEventListener('click', () => closeApprovalsModal());
        }

        if (modal) {
            modal.addEventListener('click', (e) => {
                if (e.target === modal) {
                    closeApprovalsModal();
                }
            });
        }

        // Tabs
        if (tabApprovals) {
            tabApprovals.addEventListener('click', () => switchTab('approvals'));
        }
        if (tabNotifications) {
            tabNotifications.addEventListener('click', () => switchTab('notifications'));
        }

        // Refresh & mark all read
        if (refreshBtn) {
            refreshBtn.addEventListener('click', () => {
                fetchApprovals();
                fetchNotifications();
            });
        }
        if (markAllBtn) {
            markAllBtn.addEventListener('click', () => markAllNotificationsRead());
        }

        // Global shortcut: Alt+Q for Queue / Approvals
        document.addEventListener('keydown', (e) => {
            if (e.altKey && e.key.toLowerCase() === 'q') {
                e.preventDefault();
                toggleApprovalsModal();
            } else if (e.key === 'Escape' && modal && modal.classList.contains('active')) {
                closeApprovalsModal();
            }
        });

        // Initial fetch and start polling
        fetchApprovals();
        fetchNotifications();
        pollTimer = setInterval(() => {
            fetchApprovals();
            fetchNotifications();
        }, 4000);
    }

    function toggleApprovalsModal() {
        const modal = document.getElementById('approvalsModal');
        if (!modal) return;
        if (modal.classList.contains('active')) {
            closeApprovalsModal();
        } else {
            openApprovalsModal();
        }
    }

    function openApprovalsModal(tab) {
        const modal = document.getElementById('approvalsModal');
        if (!modal) return;
        modal.classList.add('active');
        if (tab) {
            switchTab(tab);
        }
        fetchApprovals();
        fetchNotifications();
    }

    function closeApprovalsModal() {
        const modal = document.getElementById('approvalsModal');
        if (!modal) return;
        modal.classList.remove('active');
    }

    function switchTab(tab) {
        currentTab = tab;
        const tabBtnAppr = document.getElementById('tab-btn-approvals');
        const tabBtnNotif = document.getElementById('tab-btn-notifications');
        const paneAppr = document.getElementById('approvals-tab-pane');
        const paneNotif = document.getElementById('notifications-tab-pane');

        if (tab === 'approvals') {
            if (tabBtnAppr) tabBtnAppr.classList.add('active');
            if (tabBtnNotif) tabBtnNotif.classList.remove('active');
            if (paneAppr) {
                paneAppr.style.display = 'flex';
                paneAppr.classList.add('active');
            }
            if (paneNotif) {
                paneNotif.style.display = 'none';
                paneNotif.classList.remove('active');
            }
            renderApprovals();
        } else {
            if (tabBtnAppr) tabBtnAppr.classList.remove('active');
            if (tabBtnNotif) tabBtnNotif.classList.add('active');
            if (paneAppr) {
                paneAppr.style.display = 'none';
                paneAppr.classList.remove('active');
            }
            if (paneNotif) {
                paneNotif.style.display = 'flex';
                paneNotif.classList.add('active');
            }
            renderNotifications();
        }
    }

    async function fetchApprovals() {
        try {
            const res = await fetch('/api/oblivion/approvals?status=pending', {
                headers: authHeaders()
            });
            if (res.ok) {
                pendingApprovals = await res.json();
                updateBadges();
                if (currentTab === 'approvals') {
                    renderApprovals();
                }
            }
        } catch (e) {
            console.error('Failed to fetch approvals:', e);
        }
    }

    async function fetchNotifications() {
        try {
            const res = await fetch('/api/oblivion/notifications?limit=30', {
                headers: authHeaders()
            });
            if (res.ok) {
                const list = await res.json();
                // Check for new notifications to show toast
                if (isInitialized) {
                    list.forEach(notif => {
                        if (!notif.read && !seenNotificationIds.has(notif.id)) {
                            showToastNotification(notif);
                        }
                    });
                }
                notifications = list;
                list.forEach(notif => seenNotificationIds.add(notif.id));
                isInitialized = true;
                updateBadges();
                if (currentTab === 'notifications') {
                    renderNotifications();
                }
            }
        } catch (e) {
            console.error('Failed to fetch notifications:', e);
        }
    }

    function updateBadges() {
        const count = pendingApprovals.length;
        const menubarBadge = document.getElementById('menubar-approvals-badge');
        const dockBadge = document.getElementById('dock-approvals-badge');
        const tabBadge = document.getElementById('approvals-tab-badge');

        if (menubarBadge) {
            if (count > 0) {
                menubarBadge.textContent = count;
                menubarBadge.style.display = 'inline-block';
            } else {
                menubarBadge.style.display = 'none';
            }
        }

        if (dockBadge) {
            if (count > 0) {
                dockBadge.textContent = count;
                dockBadge.style.display = 'inline-block';
            } else {
                dockBadge.style.display = 'none';
            }
        }

        if (tabBadge) {
            if (count > 0) {
                tabBadge.textContent = count;
                tabBadge.style.display = 'inline-block';
            } else {
                tabBadge.style.display = 'none';
            }
        }

        // Unread notifications count
        const unreadCount = notifications.filter(n => !n.read).length;
        const notifTabBadge = document.getElementById('notifications-tab-badge');
        if (notifTabBadge) {
            if (unreadCount > 0) {
                notifTabBadge.textContent = unreadCount;
                notifTabBadge.style.display = 'inline-block';
            } else {
                notifTabBadge.style.display = 'none';
            }
        }
    }

    function renderApprovals() {
        const container = document.getElementById('approvals-cards-container');
        if (!container) return;

        if (pendingApprovals.length === 0) {
            container.innerHTML = `
                <div class="approvals-empty-state">
                    <div class="empty-icon">🛡️</div>
                    <div class="empty-text">No pending approvals</div>
                    <div class="empty-sub">Unattended tasks run safe read-only actions automatically. Risky actions pause here for one-click approval.</div>
                </div>
            `;
            return;
        }

        let html = '';
        pendingApprovals.forEach(appr => {
            html += `
                <div class="approval-card" id="approval-card-${escapeHtml(appr.id)}">
                    <div class="approval-card-top">
                        <span class="approval-tool-badge">${escapeHtml(appr.tool_name)}</span>
                        <span class="approval-time">${formatTime(appr.created_at)}</span>
                    </div>
                    <div class="approval-summary">${escapeHtml(appr.summary)}</div>
                    <div class="approval-meta">
                        <span>Session: <code>${escapeHtml(appr.session_id)}</code></span>
                        ${appr.task_id ? `<span>Task: <code>${escapeHtml(appr.task_id)}</code></span>` : ''}
                    </div>
                    <div class="approval-actions">
                        <button class="btn-approve" data-id="${escapeHtml(appr.id)}">✓ Approve</button>
                        <button class="btn-deny" data-id="${escapeHtml(appr.id)}">✕ Deny</button>
                    </div>
                </div>
            `;
        });
        container.innerHTML = html;

        // Wire click handlers for approve and deny
        container.querySelectorAll('.btn-approve').forEach(btn => {
            btn.addEventListener('click', () => {
                const id = btn.getAttribute('data-id');
                resolveApproval(id, 'approved');
            });
        });

        container.querySelectorAll('.btn-deny').forEach(btn => {
            btn.addEventListener('click', () => {
                const id = btn.getAttribute('data-id');
                resolveApproval(id, 'denied');
            });
        });
    }

    async function resolveApproval(id, decision) {
        try {
            const card = document.getElementById(`approval-card-${id}`);
            if (card) {
                card.style.opacity = '0.5';
                card.style.pointerEvents = 'none';
            }
            const res = await fetch(`/api/oblivion/approvals/${encodeURIComponent(id)}/resolve`, {
                method: 'POST',
                headers: authHeaders(),
                body: JSON.stringify({ decision })
            });
            if (res.ok) {
                // Remove from pending list locally and re-render
                pendingApprovals = pendingApprovals.filter(a => a.id !== id);
                updateBadges();
                renderApprovals();
            } else {
                console.error('Failed to resolve approval:', await res.text());
                fetchApprovals();
            }
        } catch (e) {
            console.error('Error resolving approval:', e);
            fetchApprovals();
        }
    }

    function renderNotifications() {
        const container = document.getElementById('notifications-list-container');
        if (!container) return;

        if (notifications.length === 0) {
            container.innerHTML = `
                <div class="notifications-empty-state">
                    <div class="empty-icon">🔔</div>
                    <div class="empty-text">No notifications</div>
                </div>
            `;
            return;
        }

        let html = '';
        notifications.forEach(notif => {
            html += `
                <div class="notification-item ${notif.read ? '' : 'unread'}" id="notif-item-${escapeHtml(notif.id)}">
                    <div class="notification-dot ${notif.read ? 'read' : ''}"></div>
                    <div class="notification-content">
                        <div class="notification-header">
                            <span class="notification-title">${escapeHtml(notif.title)}</span>
                            <span class="notification-time">${formatTime(notif.created_at)}</span>
                        </div>
                        <div class="notification-body">${escapeHtml(notif.body)}</div>
                    </div>
                </div>
            `;
        });
        container.innerHTML = html;

        // Clicking on an unread notification marks it as read
        container.querySelectorAll('.notification-item.unread').forEach(item => {
            item.addEventListener('click', () => {
                const id = item.id.replace('notif-item-', '');
                markNotificationRead(id);
            });
        });
    }

    async function markNotificationRead(id) {
        try {
            const res = await fetch(`/api/oblivion/notifications/${encodeURIComponent(id)}/read`, {
                method: 'POST',
                headers: authHeaders()
            });
            if (res.ok) {
                const notif = notifications.find(n => n.id === id);
                if (notif) notif.read = true;
                updateBadges();
                if (currentTab === 'notifications') {
                    renderNotifications();
                }
            }
        } catch (e) {
            console.error('Failed to mark notification read:', e);
        }
    }

    async function markAllNotificationsRead() {
        try {
            const res = await fetch('/api/oblivion/notifications/read-all', {
                method: 'POST',
                headers: authHeaders()
            });
            if (res.ok) {
                notifications.forEach(n => n.read = true);
                updateBadges();
                if (currentTab === 'notifications') {
                    renderNotifications();
                }
            }
        } catch (e) {
            console.error('Failed to mark all notifications read:', e);
        }
    }

    function showToastNotification(notif) {
        const container = document.getElementById('toast-container');
        if (!container) return;

        let icon = '🔔';
        if (notif.kind === 'approval_needed') icon = '🛡️';
        else if (notif.kind === 'run_finished') icon = '✅';
        else if (notif.kind === 'run_failed') icon = '❌';

        const toast = document.createElement('div');
        toast.className = 'toast-card';
        toast.setAttribute('data-id', notif.id);
        toast.innerHTML = `
            <div class="toast-icon">${icon}</div>
            <div class="toast-body">
                <div class="toast-title">${escapeHtml(notif.title)}</div>
                <div class="toast-msg">${escapeHtml(notif.body)}</div>
                <div class="toast-actions">
                    <button class="toast-btn primary toast-action-btn" type="button">
                        ${notif.kind === 'approval_needed' ? 'Review Approval' : 'View'}
                    </button>
                    <button class="toast-btn toast-dismiss-btn" type="button">Dismiss</button>
                </div>
            </div>
            <button class="toast-close-btn" type="button" title="Close">✕</button>
        `;

        const actionBtn = toast.querySelector('.toast-action-btn');
        const dismissBtn = toast.querySelector('.toast-dismiss-btn');
        const closeBtn = toast.querySelector('.toast-close-btn');

        function dismiss() {
            toast.style.opacity = '0';
            toast.style.transform = 'translateY(-12px) scale(0.95)';
            setTimeout(() => {
                if (toast.parentNode) toast.parentNode.removeChild(toast);
            }, 250);
        }

        if (actionBtn) {
            actionBtn.addEventListener('click', () => {
                dismiss();
                markNotificationRead(notif.id);
                openApprovalsModal(notif.kind === 'approval_needed' ? 'approvals' : 'notifications');
            });
        }

        if (dismissBtn) {
            dismissBtn.addEventListener('click', () => {
                dismiss();
                markNotificationRead(notif.id);
            });
        }

        if (closeBtn) {
            closeBtn.addEventListener('click', () => {
                dismiss();
            });
        }

        container.appendChild(toast);

        // Auto dismiss after 6 seconds
        setTimeout(() => {
            dismiss();
        }, 6000);
    }

    // Expose controller methods globally for test / interop
    window.JarvisApprovals = {
        open: openApprovalsModal,
        close: closeApprovalsModal,
        toggle: toggleApprovalsModal,
        refresh: () => {
            fetchApprovals();
            fetchNotifications();
        },
        showToast: showToastNotification,
        resolve: resolveApproval,
        markAllRead: markAllNotificationsRead
    };

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', initApprovals);
    } else {
        initApprovals();
    }
})();
