// Parinaz Kassemi Screenshot-Matched Desktop Controller for JARVIS / NEXUS

document.addEventListener('DOMContentLoaded', () => {
    initLockScreen();
    initClock();
    initThemePicker();
    initWeatherWidget();
    initDesktopIcons();
    initCustomIconAdd();
    initSplitModal();
    initSplitChat();
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
// 2. Menu Bar Clock
// ==========================================================================
function initClock() {
    const clockEl = document.getElementById('menu-clock');
    if (!clockEl) return;
    function updateClock() {
        const now = new Date();
        clockEl.textContent = now.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: true });
    }
    updateClock();
    setInterval(updateClock, 1000);
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
// 5. Desktop Icons & Dock Actions
// ==========================================================================
function initDesktopIcons() {
    const iconItems = document.querySelectorAll('.desktop-icon-item[data-action]');
    iconItems.forEach(item => {
        item.addEventListener('click', () => {
            const action = item.dataset.action;
            openSplitModalWithContext(action);
        });
    });

    // Dock app buttons
    const dockBtns = document.querySelectorAll('.dock-app-btn');
    dockBtns.forEach(btn => {
        btn.addEventListener('click', () => {
            dockBtns.forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            openSplitModal();
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

    if (chatDockBtn) {
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
    if (modal) modal.classList.add('active');
}

function openSplitModalWithContext(actionName) {
    openSplitModal();

    const taskNameEl = document.getElementById('exec-task-name');
    const logEl = document.getElementById('exec-terminal-log');

    if (taskNameEl) {
        taskNameEl.textContent = `Active Workspace: ${actionName}`;
    }

    if (logEl) {
        const timeStr = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: true });
        logEl.textContent += `\n[${timeStr}] Focused node: ${actionName}. Fetching assets...`;
        logEl.scrollTop = logEl.scrollHeight;
    }
}

// ==========================================================================
// 8. 30% Left Chat & 70% Right Execution Workspace Sync
// ==========================================================================
function initSplitChat() {
    const form = document.getElementById('split-chat-form');
    const input = document.getElementById('split-user-input');

    if (form) {
        form.addEventListener('submit', async (e) => {
            e.preventDefault();
            const message = input.value.trim();
            if (!message) return;

            appendSplitChatMsg('User', message, 'user-msg');
            input.value = '';

            const taskNameEl = document.getElementById('exec-task-name');
            const progressFill = document.getElementById('exec-progress-fill');
            const logEl = document.getElementById('exec-terminal-log');

            const nowStr = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: true });

            if (taskNameEl) taskNameEl.textContent = `Executing: "${message.slice(0, 35)}..."`;
            if (progressFill) progressFill.style.width = '35%';

            if (logEl) {
                logEl.textContent += `\n[${nowStr}] [JARVIS CORE] Processing query: "${message}"`;
                logEl.textContent += `\n[${nowStr}] [REASONING] Evaluating System 1 heuristics & System 2 logic...`;
                logEl.scrollTop = logEl.scrollHeight;
            }

            try {
                const res = await fetch('/api/chat', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ message: message })
                });
                const data = await res.json();

                appendSplitChatMsg('Jarvis AI', data.response, 'jarvis-msg');

                if (progressFill) progressFill.style.width = '100%';
                if (logEl) {
                    const doneStr = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: true });
                    logEl.textContent += `\n[${doneStr}] [SUCCESS] Task completed. Output generated.`;
                    logEl.scrollTop = logEl.scrollHeight;
                }
            } catch (err) {
                appendSplitChatMsg('Jarvis AI', 'Error executing request.', 'jarvis-msg');
                if (progressFill) progressFill.style.width = '100%';
            }
        });
    }
}

function appendSplitChatMsg(author, text, msgClass) {
    const container = document.getElementById('split-chat-messages');
    if (!container) return;

    const div = document.createElement('div');
    div.className = `msg-bubble ${msgClass}`;
    div.innerHTML = `
        <div class="msg-author">${author}</div>
        <div class="msg-text">${escapeHtml(text)}</div>
    `;

    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
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