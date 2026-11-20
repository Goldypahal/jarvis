// First-Run Setup Wizard (Q5, Month 3)
(function () {
    const wizardModal = document.getElementById("wizardModal");
    if (!wizardModal) return;

    const closeBtn = document.getElementById("close-wizard-modal-btn");
    const helpBtn = document.getElementById("menu-help-wizard");
    const step1 = document.getElementById("wizard-step-1");
    const step2 = document.getElementById("wizard-step-2");
    const step3 = document.getElementById("wizard-step-3");
    const stepBadge = document.getElementById("wizard-step-badge");

    const wsInput = document.getElementById("wizard-workspace-input");
    const groqKeyInput = document.getElementById("wizard-groq-key");
    const autostartToggle = document.getElementById("wizard-autostart-toggle");
    const testBtn = document.getElementById("wizard-test-btn");
    const testResult = document.getElementById("wizard-test-result");

    const next1Btn = document.getElementById("wizard-next-1-btn");
    const prev2Btn = document.getElementById("wizard-prev-2-btn");
    const next2Btn = document.getElementById("wizard-next-2-btn");
    const prev3Btn = document.getElementById("wizard-prev-3-btn");
    const finishBtn = document.getElementById("wizard-finish-btn");

    function getAuthHeaders() {
        const meta = document.querySelector('meta[name="jarvis-token"]');
        const token = meta ? meta.getAttribute("content") : "";
        return {
            "Content-Type": "application/json",
            "x-jarvis-token": token,
            "x-oblivion-token": token,
        };
    }

    function openWizard() {
        wizardModal.classList.add("active");
        showStep(1);
    }

    function closeWizard() {
        wizardModal.classList.remove("active");
    }

    function showStep(num) {
        if (step1) step1.style.display = num === 1 ? "flex" : "none";
        if (step2) step2.style.display = num === 2 ? "flex" : "none";
        if (step3) step3.style.display = num === 3 ? "flex" : "none";
        if (stepBadge) stepBadge.textContent = `Step ${num} of 3`;
    }

    if (closeBtn) closeBtn.addEventListener("click", closeWizard);
    if (helpBtn) helpBtn.addEventListener("click", openWizard);

    if (next1Btn) next1Btn.addEventListener("click", () => showStep(2));
    if (prev2Btn) prev2Btn.addEventListener("click", () => showStep(1));
    if (next2Btn) next2Btn.addEventListener("click", () => showStep(3));
    if (prev3Btn) prev3Btn.addEventListener("click", () => showStep(2));

    // Check status on startup; if !completed, auto-open wizard
    async function checkInitialStatus() {
        try {
            const res = await fetch("/api/system/wizard/status", {
                headers: getAuthHeaders(),
            });
            if (res.ok) {
                const data = await res.json();
                if (wsInput && !wsInput.value && data.workspace) {
                    wsInput.value = data.workspace;
                }
                if (!data.completed) {
                    openWizard();
                }
            }
        } catch (e) {
            console.error("Wizard status check failed", e);
        }
    }

    if (testBtn) {
        testBtn.addEventListener("click", async () => {
            if (!testResult) return;
            testResult.textContent = "Testing...";
            testResult.className = "wizard-test-status testing";
            try {
                const res = await fetch("/api/system/wizard/test", {
                    method: "POST",
                    headers: getAuthHeaders(),
                });
                if (res.ok) {
                    testResult.textContent = "✓ Environment and connection verified!";
                    testResult.className = "wizard-test-status success";
                } else {
                    testResult.textContent = "⚠️ Test returned status " + res.status;
                    testResult.className = "wizard-test-status warning";
                }
            } catch (e) {
                testResult.textContent = "✗ Test failed: " + e.message;
                testResult.className = "wizard-test-status error";
            }
        });
    }

    if (finishBtn) {
        finishBtn.addEventListener("click", async () => {
            finishBtn.disabled = true;
            finishBtn.textContent = "Saving...";

            const modeRadio = document.querySelector('input[name="wizard-privacy-mode"]:checked');
            const isOffline = modeRadio ? modeRadio.value === "local" : false;
            const autostart = autostartToggle ? autostartToggle.checked : false;
            const workspace = wsInput ? wsInput.value.trim() : "";
            const apiKeys = {};
            if (groqKeyInput && groqKeyInput.value.trim()) {
                apiKeys["groq"] = groqKeyInput.value.trim();
            }

            try {
                const res = await fetch("/api/system/wizard/complete", {
                    method: "POST",
                    headers: getAuthHeaders(),
                    body: JSON.stringify({
                        workspace: workspace || null,
                        offline: isOffline,
                        autostart: autostart,
                        api_keys: apiKeys,
                    }),
                });
                if (res.ok) {
                    closeWizard();
                    if (window.showNotification) {
                        window.showNotification("Jarvis setup complete! Ready for tasks.");
                    }
                } else {
                    alert("Setup save failed with status " + res.status);
                }
            } catch (e) {
                alert("Setup error: " + e.message);
            } finally {
                finishBtn.disabled = false;
                finishBtn.textContent = "Finish Setup & Launch Desktop 🎉";
            }
        });
    }

    // Check wizard status on page load
    checkInitialStatus();
})();
