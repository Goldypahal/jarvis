/**
 * Jarvis Desktop - Local Model Manager (Ollama API)
 * Handles listing installed models, streaming pull with live progress,
 * model deletion, hardware-based model recommendations, and speed testing.
 */
(function () {
    "use strict";

    let modal = null;
    let cardsContainer = null;
    let statusBadge = null;
    let countBadge = null;
    let offlineBanner = null;
    let retryBtn = null;
    let refreshBtn = null;

    // Recommendation elements
    let recSizeBadge = null;
    let recSpecsText = null;
    let recTradeoffText = null;
    let quickChips = null;

    // Pull / Download elements
    let pullInput = null;
    let pullBtn = null;
    let progressContainer = null;
    let progressModelName = null;
    let progressPctText = null;
    let progressFill = null;
    let progressStatusText = null;
    let progressBytesText = null;

    // Speed test card elements
    let speedTestCard = null;
    let closeSpeedTestBtn = null;
    let stModelName = null;
    let stTokPerSec = null;
    let stPromptTokPerSec = null;
    let stLoadTime = null;
    let stTotalTime = null;
    let stSampleText = null;

    let isPulling = false;

    // Routing & Privacy elements
    let tabBtnModels = null;
    let tabBtnRouting = null;
    let tabContentModels = null;
    let tabContentRouting = null;

    let menubarOfflineToggle = null;
    let menubarOfflineIcon = null;
    let menubarOfflineLabel = null;

    let modalOfflineToggle = null;
    let modalOfflineStatusBadge = null;
    let routingOfflineCard = null;
    let routingShieldIcon = null;
    let routingClassesContainer = null;
    let routingRefreshBtn = null;

    let routingState = {
        offline: false,
        rules: {},
        classes: [],
    };

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

    function formatBytes(bytes) {
        if (!bytes || bytes === 0) return "0 B";
        const k = 1024;
        const sizes = ["B", "KB", "MB", "GB", "TB"];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
    }

    function escapeHtml(text) {
        if (!text) return "";
        const div = document.createElement("div");
        div.textContent = text;
        return div.innerHTML;
    }

    function openModels() {
        if (!modal) return;
        modal.classList.add("active");
        fetchModels();
    }

    function closeModels() {
        if (!modal) return;
        modal.classList.remove("active");
    }

    function switchTab(tab) {
        if (!tabBtnModels || !tabBtnRouting || !tabContentModels || !tabContentRouting) return;
        if (tab === "routing") {
            tabBtnModels.classList.remove("active");
            tabBtnRouting.classList.add("active");
            tabContentModels.style.display = "none";
            tabContentModels.classList.remove("active");
            tabContentRouting.style.display = "block";
            tabContentRouting.classList.add("active");
            fetchRouting();
        } else {
            tabBtnRouting.classList.remove("active");
            tabBtnModels.classList.add("active");
            tabContentRouting.style.display = "none";
            tabContentRouting.classList.remove("active");
            tabContentModels.style.display = "block";
            tabContentModels.classList.add("active");
            fetchModels();
        }
    }

    function updateOfflineUI(isOffline) {
        routingState.offline = isOffline;

        // Menubar button
        if (menubarOfflineToggle) {
            if (isOffline) {
                menubarOfflineToggle.classList.add("active");
                if (menubarOfflineIcon) menubarOfflineIcon.textContent = "🛡️";
                if (menubarOfflineLabel) menubarOfflineLabel.textContent = "OFFLINE (Local Only)";
            } else {
                menubarOfflineToggle.classList.remove("active");
                if (menubarOfflineIcon) menubarOfflineIcon.textContent = "🌐";
                if (menubarOfflineLabel) menubarOfflineLabel.textContent = "Cloud + Local";
            }
        }

        // Modal switch
        if (modalOfflineToggle) {
            modalOfflineToggle.checked = isOffline;
        }
        if (routingOfflineCard) {
            if (isOffline) {
                routingOfflineCard.classList.add("offline-active");
            } else {
                routingOfflineCard.classList.remove("offline-active");
            }
        }
        if (routingShieldIcon) {
            routingShieldIcon.textContent = isOffline ? "🛡️" : "🌐";
        }
        if (modalOfflineStatusBadge) {
            modalOfflineStatusBadge.textContent = isOffline ? "OFFLINE ONLY" : "ONLINE";
        }
    }

    async function fetchRouting() {
        try {
            const res = await fetch("/api/routing", {
                headers: authHeaders(),
            });
            if (!res.ok) throw new Error(`HTTP ${res.status}`);
            const data = await res.json();
            routingState.offline = !!data.offline;
            routingState.rules = data.rules || {};
            routingState.classes = data.classes || [];

            updateOfflineUI(routingState.offline);
            renderRoutingClasses(routingState.classes);
        } catch (err) {
            console.error("Failed to fetch routing rules:", err);
        }
    }

    async function toggleOffline(forcedValue) {
        const nextOffline = typeof forcedValue === "boolean" ? forcedValue : !routingState.offline;
        updateOfflineUI(nextOffline);

        try {
            const res = await fetch("/api/routing", {
                method: "POST",
                headers: authHeaders(),
                body: JSON.stringify({ offline: nextOffline }),
            });
            if (!res.ok) throw new Error(`HTTP ${res.status}`);
            const data = await res.json();
            updateOfflineUI(!!data.offline);
            if (data.classes) {
                routingState.classes = data.classes;
                renderRoutingClasses(data.classes);
            }
        } catch (err) {
            console.error("Failed to update offline status:", err);
            // Revert UI to previous state on error
            updateOfflineUI(routingState.offline);
        }
    }

    function renderRoutingClasses(classes) {
        if (!routingClassesContainer) return;
        routingClassesContainer.innerHTML = "";

        if (!classes || classes.length === 0) {
            routingClassesContainer.innerHTML = '<div class="model-empty">No routing rules configured.</div>';
            return;
        }

        const icons = {
            private: "🔒",
            simple: "⚡",
            hard: "🧠",
            vision: "👁️",
            long_context: "📚",
        };

        classes.forEach(function (cls) {
            const card = document.createElement("div");
            card.className = "routing-class-card" + (cls.local_only ? " private-class" : "");

            const icon = icons[cls.id] || "📌";
            const badgeClass = cls.local_only ? "routing-class-badge local-only" : "routing-class-badge";
            const badgeText = cls.local_only ? "🛡️ Local Only (Forced)" : "Cloud + Local";

            const patterns = Array.isArray(cls.patterns) ? cls.patterns : [];
            const patternTags = patterns
                .map(function (pat) {
                    const isLocal = pat.includes("ollama") || pat.includes("local");
                    const tagCls = isLocal ? "routing-target-tag local-target" : "routing-target-tag";
                    return `<span class="${tagCls}">${escapeHtml(pat)}</span>`;
                })
                .join("");

            card.innerHTML = `
                <div class="routing-class-header">
                    <div class="routing-class-title-row">
                        <span class="routing-class-icon">${icon}</span>
                        <span class="routing-class-label">${escapeHtml(cls.label)}</span>
                    </div>
                    <span class="${badgeClass}">${badgeText}</span>
                </div>
                <p class="routing-class-desc">${escapeHtml(cls.description)}</p>
                <div class="routing-targets-row">
                    <span class="routing-targets-label">Target Priority:</span>
                    ${patternTags || '<span style="color: rgba(0,0,0,0.4);">None configured</span>'}
                </div>
            `;

            routingClassesContainer.appendChild(card);
        });
    }

    async function fetchModels() {
        if (!cardsContainer) return;

        if (statusBadge) statusBadge.textContent = "checking...";

        try {
            const res = await fetch("/api/models", {
                headers: authHeaders(),
            });

            if (!res.ok) {
                throw new Error(`HTTP ${res.status}`);
            }

            const data = await res.json();
            renderStatus(data.status);
            renderRecommendation(data.recommendation);
            renderModels(data.models || []);
        } catch (err) {
            console.error("Failed to fetch models:", err);
            if (statusBadge) {
                statusBadge.textContent = "Error";
                statusBadge.style.color = "#ff3b30";
            }
            if (offlineBanner) offlineBanner.style.display = "flex";
            renderModels([]);
        }
    }

    function renderStatus(status) {
        if (!status) return;

        if (status.running) {
            if (statusBadge) {
                statusBadge.textContent = `Online (v${status.version || "ok"})`;
                statusBadge.style.color = "#34c759";
            }
            if (offlineBanner) offlineBanner.style.display = "none";
        } else {
            if (statusBadge) {
                statusBadge.textContent = "Offline";
                statusBadge.style.color = "#ff9500";
            }
            if (offlineBanner) {
                offlineBanner.style.display = "flex";
                const epLbl = document.getElementById("models-endpoint-lbl");
                if (epLbl) epLbl.textContent = status.endpoint || "http://localhost:11434";
            }
        }
    }

    function renderRecommendation(rec) {
        if (!rec) return;

        if (recSizeBadge) {
            recSizeBadge.textContent = rec.recommended_size || "--";
        }
        if (recSpecsText) {
            const vramText = rec.fits_in_vram
                ? "fits fully in VRAM"
                : `requires ~${rec.estimated_ram_required_gb || "?"} GB system RAM`;
            recSpecsText.textContent = `Quantization: ${rec.recommended_quantization || "Q4_K_M"} (${vramText})`;
        }
        if (recTradeoffText) {
            recTradeoffText.textContent = rec.tradeoff_explanation || "";
        }

        if (quickChips && Array.isArray(rec.recommended_models)) {
            quickChips.innerHTML = "";
            rec.recommended_models.forEach(function (modelName) {
                const chip = document.createElement("button");
                chip.type = "button";
                chip.className = "quick-chip";
                chip.textContent = `+ ${modelName}`;
                chip.title = `Click to download ${modelName}`;
                chip.addEventListener("click", function () {
                    if (pullInput) {
                        pullInput.value = modelName;
                        pullInput.focus();
                    }
                });
                quickChips.appendChild(chip);
            });

            // Also offer nomic-embed-text for local embeddings if not present
            if (!rec.recommended_models.includes("nomic-embed-text")) {
                const embedChip = document.createElement("button");
                embedChip.type = "button";
                embedChip.className = "quick-chip";
                embedChip.textContent = "+ nomic-embed-text (embeddings)";
                embedChip.title = "Local embedding model for long-term memory";
                embedChip.addEventListener("click", function () {
                    if (pullInput) {
                        pullInput.value = "nomic-embed-text";
                        pullInput.focus();
                    }
                });
                quickChips.appendChild(embedChip);
            }
        }
    }

    function renderModels(models) {
        if (!cardsContainer) return;
        cardsContainer.innerHTML = "";

        if (countBadge) {
            countBadge.textContent = `${models.length} model${models.length === 1 ? "" : "s"}`;
        }

        if (models.length === 0) {
            cardsContainer.innerHTML = `
                <div class="sessions-empty-state" style="padding: 30px 10px;">
                    <div class="empty-icon">🤖</div>
                    <div class="empty-text">No local models installed in Ollama yet.</div>
                    <div style="font-size: 12px; color: rgba(0,0,0,0.5);">Use the download form above or click a recommended model chip.</div>
                </div>
            `;
            return;
        }

        models.forEach(function (m) {
            const card = document.createElement("div");
            card.className = "model-card";

            const details = m.details || {};
            const paramSize = details.parameter_size || "";
            const quant = details.quantization_level || "";
            const family = details.family || "";
            const format = details.format || "gguf";

            const tags = [format, family, paramSize, quant].filter(Boolean).join(" · ");

            card.innerHTML = `
                <div class="model-info-col">
                    <div class="model-name-row">
                        <span class="model-name">${escapeHtml(m.name)}</span>
                        ${tags ? `<span class="model-tag">${escapeHtml(tags)}</span>` : ""}
                    </div>
                    <div class="model-meta-row">
                        <span>Disk size: <strong>${escapeHtml(m.size_formatted || formatBytes(m.size))}</strong></span>
                        ${m.modified_at ? `<span>Modified: ${new Date(m.modified_at).toLocaleDateString()}</span>` : ""}
                    </div>
                </div>
                <div class="model-actions-col">
                    <button class="mem-btn-sm btn-speed-test" data-model="${escapeHtml(m.name)}" title="Test inference speed">⚡ Speed Test</button>
                    <button class="mem-btn-sm mem-btn-warn btn-delete-model" data-model="${escapeHtml(m.name)}" title="Delete this model">🗑️ Delete</button>
                </div>
            `;

            // Speed test button
            const testBtn = card.querySelector(".btn-speed-test");
            if (testBtn) {
                testBtn.addEventListener("click", function () {
                    runSpeedTest(m.name, testBtn);
                });
            }

            // Delete button
            const delBtn = card.querySelector(".btn-delete-model");
            if (delBtn) {
                delBtn.addEventListener("click", function () {
                    deleteModel(m.name);
                });
            }

            cardsContainer.appendChild(card);
        });
    }

    async function deleteModel(modelName) {
        if (!confirm(`Are you sure you want to delete local model '${modelName}'? This frees disk space immediately.`)) {
            return;
        }

        try {
            const res = await fetch("/api/models/delete", {
                method: "POST",
                headers: authHeaders(),
                body: JSON.stringify({ name: modelName }),
            });

            if (!res.ok) {
                throw new Error(`HTTP ${res.status}`);
            }

            const data = await res.json();
            if (data.status === "ok") {
                fetchModels();
            } else {
                alert(`Delete failed: ${data.message || "Unknown error"}`);
            }
        } catch (err) {
            alert(`Error deleting model: ${err.message}`);
        }
    }

    async function runSpeedTest(modelName, buttonEl) {
        if (buttonEl) {
            buttonEl.disabled = true;
            buttonEl.textContent = "⏳ Running...";
        }

        try {
            const res = await fetch("/api/models/speed-test", {
                method: "POST",
                headers: authHeaders(),
                body: JSON.stringify({ model: modelName }),
            });

            if (!res.ok) {
                throw new Error(`HTTP ${res.status}`);
            }

            const data = await res.json();
            if (data.status === "ok" && data.result) {
                displaySpeedTestResult(data.result);
            } else {
                alert(`Speed test failed: ${data.error || "Unknown error"}`);
            }
        } catch (err) {
            alert(`Speed test error: ${err.message}`);
        } finally {
            if (buttonEl) {
                buttonEl.disabled = false;
                buttonEl.textContent = "⚡ Speed Test";
            }
        }
    }

    function displaySpeedTestResult(result) {
        if (!speedTestCard) return;

        if (stModelName) stModelName.textContent = result.model;
        if (stTokPerSec) stTokPerSec.textContent = `${result.tokens_per_second.toFixed(1)} tok/s`;
        if (stPromptTokPerSec) stPromptTokPerSec.textContent = `${result.prompt_eval_tokens_per_second.toFixed(1)} tok/s`;
        if (stLoadTime) stLoadTime.textContent = `${result.load_duration_ms.toFixed(0)} ms`;
        if (stTotalTime) stTotalTime.textContent = `${result.total_duration_ms.toFixed(0)} ms`;
        if (stSampleText) stSampleText.textContent = `"${result.sample_text}"`;

        speedTestCard.style.display = "flex";
        speedTestCard.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }

    async function startPull(modelName) {
        if (isPulling) return;
        if (!modelName || !modelName.trim()) {
            alert("Please enter a valid model name (e.g. qwen2.5-coder:7b).");
            return;
        }

        const name = modelName.trim();
        isPulling = true;
        if (pullBtn) pullBtn.disabled = true;
        if (pullInput) pullInput.disabled = true;

        if (progressContainer) progressContainer.style.display = "flex";
        if (progressModelName) progressModelName.textContent = `Pulling ${name}...`;
        if (progressPctText) progressPctText.textContent = "0%";
        if (progressFill) progressFill.style.width = "0%";
        if (progressStatusText) progressStatusText.textContent = "Requesting download from Ollama...";
        if (progressBytesText) progressBytesText.textContent = "";

        try {
            const response = await fetch("/api/models/pull", {
                method: "POST",
                headers: authHeaders(),
                body: JSON.stringify({ name }),
            });

            if (!response.ok) {
                throw new Error(`HTTP ${response.status}`);
            }

            const reader = response.body.getReader();
            const decoder = new TextDecoder("utf-8");
            let buffer = "";

            while (true) {
                const { done, value } = await reader.read();
                if (done) break;

                buffer += decoder.decode(value, { stream: true });
                const lines = buffer.split("\n");
                buffer = lines.pop(); // keep remainder

                for (const line of lines) {
                    const trimmed = line.trim();
                    if (!trimmed || trimmed.startsWith(":")) continue;

                    let jsonStr = trimmed;
                    if (trimmed.startsWith("data:")) {
                        jsonStr = trimmed.slice(5).trim();
                    }

                    try {
                        const progress = JSON.parse(jsonStr);
                        updatePullProgress(progress);
                    } catch {
                        // ignore malformed line
                    }
                }
            }

            if (progressStatusText) progressStatusText.textContent = "Download complete!";
            if (progressPctText) progressPctText.textContent = "100%";
            if (progressFill) progressFill.style.width = "100%";

            setTimeout(function () {
                if (progressContainer) progressContainer.style.display = "none";
                fetchModels();
            }, 1800);
        } catch (err) {
            console.error("Pull failed:", err);
            if (progressStatusText) progressStatusText.textContent = `Failed: ${err.message}`;
            alert(`Model download error: ${err.message}`);
        } finally {
            isPulling = false;
            if (pullBtn) pullBtn.disabled = false;
            if (pullInput) {
                pullInput.disabled = false;
                pullInput.value = "";
            }
        }
    }

    function updatePullProgress(progress) {
        if (!progress) return;

        if (progress.error) {
            if (progressStatusText) progressStatusText.textContent = `Error: ${progress.error}`;
            return;
        }

        if (progress.status && progressStatusText) {
            progressStatusText.textContent = progress.status;
        }

        if (typeof progress.percent === "number") {
            const pct = Math.min(100, Math.max(0, progress.percent));
            if (progressPctText) progressPctText.textContent = `${pct.toFixed(1)}%`;
            if (progressFill) progressFill.style.width = `${pct}%`;
        }

        if (progress.completed && progress.total && progressBytesText) {
            progressBytesText.textContent = `${formatBytes(progress.completed)} / ${formatBytes(progress.total)}`;
        }
    }

    function init() {
        modal = document.getElementById("modelsModal");
        if (!modal) return;

        cardsContainer = document.getElementById("models-cards-container");
        statusBadge = document.getElementById("models-status-badge");
        countBadge = document.getElementById("models-count-badge");
        offlineBanner = document.getElementById("models-offline-banner");
        retryBtn = document.getElementById("models-retry-btn");
        refreshBtn = document.getElementById("models-refresh-btn");

        recSizeBadge = document.getElementById("models-rec-size-badge");
        recSpecsText = document.getElementById("models-rec-specs-text");
        recTradeoffText = document.getElementById("models-rec-tradeoff-text");
        quickChips = document.getElementById("models-quick-chips");

        pullInput = document.getElementById("model-pull-input");
        pullBtn = document.getElementById("model-pull-btn");
        progressContainer = document.getElementById("model-pull-progress-container");
        progressModelName = document.getElementById("model-pull-model-name");
        progressPctText = document.getElementById("model-pull-pct-text");
        progressFill = document.getElementById("model-pull-progress-fill");
        progressStatusText = document.getElementById("model-pull-status-text");
        progressBytesText = document.getElementById("model-pull-bytes-text");

        speedTestCard = document.getElementById("model-speed-test-card");
        closeSpeedTestBtn = document.getElementById("close-speed-test-card-btn");
        stModelName = document.getElementById("st-model-name");
        stTokPerSec = document.getElementById("st-tok-per-sec");
        stPromptTokPerSec = document.getElementById("st-prompt-tok-per-sec");
        stLoadTime = document.getElementById("st-load-time");
        stTotalTime = document.getElementById("st-total-time");
        stSampleText = document.getElementById("st-sample-text");

        // Routing & Tab navigation elements
        tabBtnModels = document.getElementById("tab-btn-models");
        tabBtnRouting = document.getElementById("tab-btn-routing");
        tabContentModels = document.getElementById("tab-content-models");
        tabContentRouting = document.getElementById("tab-content-routing");

        menubarOfflineToggle = document.getElementById("menubar-offline-toggle");
        menubarOfflineIcon = document.getElementById("menubar-offline-icon");
        menubarOfflineLabel = document.getElementById("menubar-offline-label");

        modalOfflineToggle = document.getElementById("modal-offline-toggle");
        modalOfflineStatusBadge = document.getElementById("modal-offline-status-badge");
        routingOfflineCard = document.querySelector(".routing-offline-card");
        routingShieldIcon = document.getElementById("routing-shield-icon");
        routingClassesContainer = document.getElementById("routing-classes-container");
        routingRefreshBtn = document.getElementById("routing-refresh-btn");

        if (tabBtnModels) {
            tabBtnModels.addEventListener("click", function () {
                switchTab("models");
            });
        }
        if (tabBtnRouting) {
            tabBtnRouting.addEventListener("click", function () {
                switchTab("routing");
            });
        }

        if (menubarOfflineToggle) {
            menubarOfflineToggle.addEventListener("click", function () {
                toggleOffline();
            });
        }

        if (modalOfflineToggle) {
            modalOfflineToggle.addEventListener("change", function (e) {
                toggleOffline(e.target.checked);
            });
        }

        if (routingRefreshBtn) {
            routingRefreshBtn.addEventListener("click", fetchRouting);
        }

        // Close triggers
        const closeBtn = document.getElementById("close-models-modal-btn");
        if (closeBtn) closeBtn.addEventListener("click", closeModels);

        if (closeSpeedTestBtn) {
            closeSpeedTestBtn.addEventListener("click", function () {
                if (speedTestCard) speedTestCard.style.display = "none";
            });
        }

        modal.addEventListener("click", function (e) {
            if (e.target === modal) closeModels();
        });

        // Open triggers
        const dockBtn = document.getElementById("open-models-dock-btn");
        if (dockBtn) dockBtn.addEventListener("click", openModels);

        const widgetBtn = document.getElementById("open-models-widget-btn");
        if (widgetBtn) widgetBtn.addEventListener("click", openModels);

        const menuBtn = document.getElementById("menu-models-btn");
        if (menuBtn) menuBtn.addEventListener("click", openModels);

        if (retryBtn) retryBtn.addEventListener("click", fetchModels);
        if (refreshBtn) refreshBtn.addEventListener("click", fetchModels);

        // Pull form submission
        if (pullBtn && pullInput) {
            pullBtn.addEventListener("click", function () {
                startPull(pullInput.value);
            });

            pullInput.addEventListener("keydown", function (e) {
                if (e.key === "Enter") {
                    e.preventDefault();
                    startPull(pullInput.value);
                }
            });
        }

        // Global hotkey Ctrl+L to toggle Models modal
        document.addEventListener("keydown", function (e) {
            if ((e.ctrlKey || e.metaKey) && (e.key === "l" || e.key === "L")) {
                const target = e.target;
                if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) {
                    return;
                }
                e.preventDefault();
                if (modal.classList.contains("active")) {
                    closeModels();
                } else {
                    openModels();
                }
            } else if (e.key === "Escape" && modal.classList.contains("active")) {
                closeModels();
            }
        });

        window.openModelsModal = openModels;
        window.closeModelsModal = closeModels;
        window.toggleOfflineMode = toggleOffline;

        // Load initial routing & offline status so menubar indicator is live
        fetchRouting();
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
