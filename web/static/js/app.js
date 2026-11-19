// Jarvis Desktop Controller for Oblivion Agent

document.addEventListener('DOMContentLoaded', () => {
    initLockScreen();
    initClock();
    initThemePicker();
    initWeatherWidget();
    initDesktopIcons();
    initCustomIconAdd();
    initSplitModal();
    initDesktopWidgets();
});

// ==========================================================================
// 1. Lock Screen (#loader)
// ==========================================================================
function initLockScreen() {
    const loader = document.getElementById('loader');
    const unlockBtn = document.getElementById('unlock-btn');

    function unlock() {
        if (loader) loader.classList.add('hide');
    }

    if (unlockBtn) unlockBtn.addEventListener('click', unlock);
    if (loader) {
        loader.addEventListener('click', (e) => {
            if (e.target === loader || e.target.classList.contains('lock-center')) {
                unlock();
            }
        });
    }
}

// ==========================================================================
// 2. Menu Bar Clock & Live Date (B7)
// ==========================================================================
function initClock() {
    const clockEl = document.getElementById('menu-clock');
    const dateEl = document.getElementById('menu-date');
    if (!clockEl && !dateEl) return;
    function updateClockAndDate() {
        const now = new Date();
        if (clockEl) {
            clockEl.textContent = now.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: true });
        }
        if (dateEl) {
            dateEl.textContent = now.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
        }
    }
    updateClockAndDate();
    setInterval(updateClockAndDate, 1000);
}

// ==========================================================================
// 3. Dynamic Weather Widget (Chandigarh Default / Geolocation)
// ==========================================================================
function getWmoWeatherInfo(code) {
    if (code === 0) return { icon: '☀️', text: 'Clear' };
    if ([1, 2, 3].includes(code)) return { icon: '🌤️', text: 'Partly Cloudy' };
    if ([45, 48].includes(code)) return { icon: '🌫️', text: 'Fog/Haze' };
    if ([51, 53, 55, 61, 63, 65].includes(code)) return { icon: '🌧️', text: 'Rain' };
    if ([71, 73, 75, 77].includes(code)) return { icon: '❄️', text: 'Snow' };
    if ([80, 81, 82, 95, 96, 99].includes(code)) return { icon: '⛈️', text: 'Thunderstorm' };
    return { icon: '☀️', text: 'Clear' };
}

async function fetchWeather(lat, lon, cityName, nameElId, tempElId, condElId) {
    try {
        const url = `https://api.open-meteo.com/v1/forecast?latitude=${lat}&longitude=${lon}&current_weather=true`;
        const res = await fetch(url);
        const data = await res.json();

        if (data && data.current_weather) {
            const temp = Math.round(data.current_weather.temperature);
            const code = data.current_weather.weathercode;
            const info = getWmoWeatherInfo(code);

            const nameEl = document.getElementById(nameElId);
            const tempEl = document.getElementById(tempElId);
            const condEl = document.getElementById(condElId);

            if (nameEl) nameEl.textContent = `${cityName.toUpperCase()} ▾`;
            if (tempEl) tempEl.textContent = `${temp}°C`;
            if (condEl) condEl.textContent = `${info.icon} ${info.text}`;
        }
    } catch (err) {
        console.error(`Weather fetch error for ${cityName}:`, err);
    }
}

function initWeatherWidget() {
    const btn = document.getElementById('enable-weather-btn');
    const optIn = document.getElementById('weather-opt-in');
    const dataBox = document.getElementById('weather-data');
    if (!btn) return;

    btn.addEventListener('click', () => {
        if (optIn) optIn.style.display = 'none';
        if (dataBox) dataBox.style.display = 'grid';
        loadWeather();
    });
}

function loadWeather() {
    // Column 1: CHANDIGARH (Default & Reference)
    fetchWeather(30.7333, 76.7794, 'CHANDIGARH', 'city-1-name', 'city-1-temp', 'city-1-cond');

    // Column 2: User Location via Geolocation (or fallback to NEW DELHI)
    if ('geolocation' in navigator) {
        navigator.geolocation.getCurrentPosition(
            async (pos) => {
                const lat = pos.coords.latitude;
                const lon = pos.coords.longitude;

                let city = 'MY LOCATION';
                try {
                    const geoRes = await fetch(`https://nominatim.openstreetmap.org/reverse?format=json&lat=${lat}&lon=${lon}`);
                    const geoData = await geoRes.json();
                    if (geoData && geoData.address) {
                        city = (geoData.address.city || geoData.address.town || geoData.address.state_district || 'MY LOCATION').toUpperCase();
                    }
                } catch (e) {
                    // ignore reverse geocode error
                }

                fetchWeather(lat, lon, city, 'city-2-name', 'city-2-temp', 'city-2-cond');
            },
            (err) => {
                fetchWeather(28.6139, 77.2090, 'NEW DELHI', 'city-2-name', 'city-2-temp', 'city-2-cond');
            },
            { timeout: 6000 }
        );
    } else {
        fetchWeather(28.6139, 77.2090, 'NEW DELHI', 'city-2-name', 'city-2-temp', 'city-2-cond');
    }
}

// ==========================================================================
// 4. Background Theme Picker (4 Color Circles)
// ==========================================================================
function initThemePicker() {
    const circles = document.querySelectorAll('.theme-circle');
    circles.forEach(circle => {
        circle.addEventListener('click', () => {
            circles.forEach(c => c.classList.remove('active'));
            circle.classList.add('active');

            const theme = circle.dataset.theme;
            if (theme) {
                document.body.setAttribute('data-theme', theme);
            }
        });
    });
}

// ==========================================================================
// 5. Desktop Icons & Dock Actions (B7)
// ==========================================================================
function initDesktopIcons() {
    const iconItems = document.querySelectorAll('.desktop-icon-item[data-action]');
    iconItems.forEach(item => {
        item.addEventListener('click', () => {
            const action = item.dataset.action;
            const categoryMap = {
                'open-research': 'paper',
                'open-pictures': 'picture',
                'open-docs': 'document',
                'open-notebook': 'notebook',
                'open-casestudies': 'case-study'
            };
            if (categoryMap[action] && window.FilesManager?.openWithFilter) {
                window.FilesManager.openWithFilter(categoryMap[action]);
            } else {
                openSplitModalWithContext(action);
            }
        });
    });

    // Bio header link
    const bioLearnMore = document.getElementById('bio-learn-more');
    if (bioLearnMore) {
        bioLearnMore.addEventListener('click', (e) => {
            e.preventDefault();
            openSplitModalWithContext('jarvis-architecture');
        });
    }

    // Dock app buttons (excluding chat dock btn which toggles Oblivion panel)
    const dockBtns = document.querySelectorAll('.dock-app-btn:not(#open-chat-dock-btn)');
    dockBtns.forEach(btn => {
        btn.addEventListener('click', () => {
            dockBtns.forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            if (btn.dataset.action === 'files' && window.FilesManager?.open) {
                window.FilesManager.open('all');
            } else if (btn.dataset.action) {
                openSplitModalWithContext(btn.dataset.action);
            } else {
                openSplitModal();
            }
        });
    });
}

// ==========================================================================
// 6. Add Custom Node (+ Button)
// ==========================================================================
function initCustomIconAdd() {
    const addBtn = document.getElementById('add-custom-icon-btn');
    const grid = document.getElementById('icons-grid');

    if (addBtn && grid) {
        addBtn.addEventListener('click', () => {
            const name = prompt('Enter custom node name:', 'New Document');
            if (!name) return;
            const emoji = prompt('Enter emoji/icon:', '📌') || '📌';

            const newItem = document.createElement('div');
            newItem.className = 'desktop-icon-item';
            newItem.dataset.action = 'custom-' + Date.now();
            newItem.innerHTML = `
                <div class="icon-img-box">${emoji}</div>
                <span class="icon-label">${escapeHtml(name)}</span>
            `;

            newItem.addEventListener('click', () => {
                openSplitModalWithContext(name);
            });

            grid.insertBefore(newItem, addBtn);
        });
    }
}

// ==========================================================================
// 7. 30/70 Percent Split Window Modal (#splitModal)
// ==========================================================================
function initSplitModal() {
    const modal = document.getElementById('splitModal');
    const closeBtn = document.getElementById('close-split-modal-btn');
    const chatDockBtn = document.getElementById('open-chat-dock-btn');

    // The dock's chat icon opens the Oblivion panel (static/js/oblivion.js);
    // without it, it falls back to the 30/70 workspace window.
    if (chatDockBtn && !window.OblivionPanel) {
        chatDockBtn.addEventListener('click', openSplitModal);
    }

    if (closeBtn && modal) {
        closeBtn.addEventListener('click', () => {
            modal.classList.remove('active');
        });
    }

    if (modal) {
        modal.addEventListener('click', (e) => {
            if (e.target === modal) {
                modal.classList.remove('active');
            }
        });
    }
}

function openSplitModal() {
    const modal = document.getElementById('splitModal');
    if (modal) {
        modal.classList.add('active');
        if (window.Oblivion?.ensureStarted) window.Oblivion.ensureStarted();
        const input = document.getElementById('split-user-input');
        if (input) setTimeout(() => input.focus(), 200);
    }
}

function openSplitModalWithContext(actionName) {
    openSplitModal();
    const input = document.getElementById('split-user-input');
    if (input) {
        const promptMap = {
            'terminal': 'Run command: ',
            'files': 'List files in workspace',
            'web': 'Search the web for ',
            'skills': 'List skills',
            'mcp': 'Show connected MCP servers',
            'jarvis-architecture': 'Tell me about the Jarvis desktop and Oblivion engine architecture',
            'open-research': 'Research topic: ',
            'open-pictures': 'Generate picture: ',
            'open-docs': 'Summarize document: ',
            'open-notebook': 'Take notes on: ',
            'open-casestudies': 'Analyze case study: '
        };
        input.value = promptMap[actionName] || `Explain the ${actionName} workflow`;
        input.focus();
    }
}

function escapeHtml(str) {
    return str.replace(/[&<>"']/g, function(m) {
        return {
            '&': '&amp;',
            '<': '&lt;',
            '>': '&gt;',
            '"': '&quot;',
            "'": '&#039;'
        }[m];
    });
}

window.openSplitModal = openSplitModal;
window.openSplitModalWithContext = openSplitModalWithContext;
window.escapeHtml = escapeHtml;

// ==========================================================================
// 8. Real Desktop Widgets Controller (B7)
// ==========================================================================
function initDesktopWidgets() {
    const openMemBtn = document.getElementById('open-memory-widget-btn');
    if (openMemBtn) {
        openMemBtn.addEventListener('click', () => {
            if (window.MemoryModal?.open) window.MemoryModal.open();
        });
    }

    const openFilesBtn = document.getElementById('open-files-widget-btn');
    if (openFilesBtn) {
        openFilesBtn.addEventListener('click', () => {
            if (window.FilesManager?.open) window.FilesManager.open('all');
        });
    }

    fetchAndRenderDashboard();
    setInterval(fetchAndRenderDashboard, 10000);
}

async function fetchAndRenderDashboard() {
    try {
        const token = window.Oblivion?.token || new URLSearchParams(window.location.search).get('t') || '';
        const headers = token ? { 'x-oblivion-token': token } : {};
        const res = await fetch('/api/oblivion/dashboard', { headers });
        if (!res.ok) return;
        const data = await res.json();
        renderDashboardData(data);
    } catch (err) {
        console.error('Failed to fetch dashboard metrics:', err);
    }
}

function renderDashboardData(data) {
    if (!data) return;

    // 1. Usage Today
    const totalReqEl = document.getElementById('usage-total-requests');
    const totalTokensEl = document.getElementById('usage-total-tokens');
    const breakdownList = document.getElementById('usage-breakdown-list');

    const usageList = data.usage_today || [];
    let sumReq = 0;
    let sumTokens = 0;
    usageList.forEach(u => {
        sumReq += u.requests || 0;
        sumTokens += (u.prompt_tokens || 0) + (u.completion_tokens || 0);
    });

    if (totalReqEl) totalReqEl.textContent = `${sumReq} req`;
    if (totalTokensEl) totalTokensEl.textContent = sumTokens.toLocaleString();

    if (breakdownList) {
        if (usageList.length === 0) {
            breakdownList.innerHTML = '<div class="empty-widget-msg">No requests recorded today</div>';
        } else {
            breakdownList.innerHTML = usageList.map(u => {
                const total = (u.prompt_tokens || 0) + (u.completion_tokens || 0);
                const shortTarget = (u.target || '').replace(/^.*\//, '');
                return `
                    <div class="usage-row-item">
                        <span class="usage-target" title="${escapeHtml(u.target)}">${escapeHtml(shortTarget)}</span>
                        <span class="usage-tokens">${total.toLocaleString()} tok (${u.requests} req)</span>
                    </div>
                `;
            }).join('');
        }
    }

    // 2. Current Tasks
    const tasksProgress = document.getElementById('tasks-progress-lbl');
    const tasksBar = document.getElementById('tasks-widget-bar');
    const tasksList = document.getElementById('tasks-widget-list');

    const tasks = data.tasks || [];
    if (tasks.length === 0) {
        if (tasksProgress) tasksProgress.textContent = '0/0';
        if (tasksBar) tasksBar.style.width = '0%';
        if (tasksList) tasksList.innerHTML = '<li class="empty-widget-msg">No active tasks</li>';
    } else {
        const completed = tasks.filter(t => t.status === 'completed').length;
        const pct = Math.round((completed / tasks.length) * 100);
        if (tasksProgress) tasksProgress.textContent = `${completed}/${tasks.length}`;
        if (tasksBar) tasksBar.style.width = `${pct}%`;
        if (tasksList) {
            tasksList.innerHTML = tasks.slice(0, 4).map(t => {
                const isDone = t.status === 'completed';
                const isProg = t.status === 'in_progress';
                const icon = isDone ? '✓' : (isProg ? '▶' : '○');
                const cls = isDone ? 'done' : (isProg ? 'in-prog' : 'pending');
                return `
                    <li class="task-widget-item ${cls}">
                        <span class="task-status-icon">${icon}</span>
                        <span class="task-text" title="${escapeHtml(t.text)}">${escapeHtml(t.text)}</span>
                    </li>
                `;
            }).join('');
        }
    }

    // 3. Files Created This Week
    const filesCountLbl = document.getElementById('files-count-lbl');
    const filesList = document.getElementById('files-widget-list');
    const recentFiles = data.recent_files || [];

    if (filesCountLbl) filesCountLbl.textContent = `${recentFiles.length} files`;
    if (filesList) {
        if (recentFiles.length === 0) {
            filesList.innerHTML = '<div class="empty-widget-msg">No files created this week</div>';
        } else {
            filesList.innerHTML = recentFiles.slice(0, 4).map(f => {
                const catIcon = {
                    paper: '📄',
                    picture: '🎨',
                    document: '📁',
                    notebook: '📓',
                    'case-study': '📂',
                    code: '💻'
                }[f.category] || '📄';
                return `
                    <div class="file-widget-row" data-path="${escapeHtml(f.path)}">
                        <span class="file-icon">${catIcon}</span>
                        <span class="file-name" title="${escapeHtml(f.path)}">${escapeHtml(f.name)}</span>
                        <span class="file-cat">${escapeHtml(f.category)}</span>
                    </div>
                `;
            }).join('');

            filesList.querySelectorAll('.file-widget-row').forEach(row => {
                row.addEventListener('click', () => {
                    if (window.FilesManager?.open) window.FilesManager.open('all');
                });
            });
        }
    }

    // 4. Long-Term Memory
    const memCountLbl = document.getElementById('memory-widget-count');
    const memBreakdown = document.getElementById('memory-widget-breakdown');
    const memCount = data.memories_count || 0;
    const byType = data.memories_by_type || {};

    if (memCountLbl) memCountLbl.textContent = `${memCount} items`;
    if (memBreakdown) {
        if (memCount === 0) {
            memBreakdown.innerHTML = '<div class="empty-widget-msg">No memories stored</div>';
        } else {
            const labelMap = {
                user_pref: 'pref',
                project_fact: 'project',
                recipe: 'recipe',
                fact: 'fact',
                past_outcome: 'outcome'
            };
            const pills = Object.entries(byType).map(([type, count]) => {
                return `<span class="mem-type-pill">${count} ${labelMap[type] || type}</span>`;
            }).join(' ');
            memBreakdown.innerHTML = `<div class="mem-pills-row">${pills}</div>`;
        }
    }

    // 5. Engine & Model Status
    const engineDot = document.getElementById('engine-status-dot');
    const engineVer = document.getElementById('engine-ver-lbl');
    const engineState = document.getElementById('engine-state-lbl');
    const modelBadge = document.getElementById('engine-model-badge');
    const wsPath = document.getElementById('engine-workspace-path');

    const eng = data.engine || {};
    if (engineVer && eng.version) engineVer.textContent = `Oblivion v${eng.version}`;
    if (engineState) {
        const isReady = eng.status === 'ready';
        const isBusy = eng.status === 'busy';
        engineState.textContent = isBusy ? 'Busy' : (isReady ? 'Ready' : 'Offline');
        if (engineDot) {
            engineDot.className = `status-indicator-dot ${isBusy ? 'busy' : (isReady ? 'ready' : 'offline')}`;
        }
    }
    if (modelBadge) modelBadge.textContent = eng.model || 'groq:llama-3.3-70b';
    if (wsPath && eng.workspace) wsPath.textContent = eng.workspace;

    // Hardware & Recommendation specs
    const ramText = document.getElementById('engine-ram-text');
    const gpuText = document.getElementById('engine-gpu-text');
    const recText = document.getElementById('engine-rec-text');
    const recSpec = document.getElementById('engine-rec-spec');

    if (eng.hardware) {
        if (ramText) {
            ramText.textContent = `${eng.hardware.total_ram_gb} GB RAM (${eng.hardware.available_ram_gb} GB free)`;
        }
        if (gpuText) {
            const vramStr = eng.hardware.vram_gb ? ` (${eng.hardware.vram_gb} GB)` : '';
            gpuText.textContent = `${eng.hardware.gpu_name}${vramStr}`;
        }
    }
    if (eng.recommendation) {
        if (recText) {
            const firstModel = eng.recommendation.recommended_models && eng.recommendation.recommended_models[0]
                ? ` (${eng.recommendation.recommended_models[0]})`
                : '';
            recText.textContent = `Recommended: ${eng.recommendation.recommended_size}${firstModel}`;
        }
        if (recSpec && eng.recommendation.tradeoff_explanation) {
            recSpec.title = eng.recommendation.tradeoff_explanation;
        }
    }
}
