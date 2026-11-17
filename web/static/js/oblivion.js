// Oblivion panel for the Jarvis desktop.
//
// Opens from the dock's chat icon, the "Oblivion" item in the menu bar, or Ctrl+J
// (Cmd+J on a Mac). The Jarvis server starts the Oblivion agent the first time the
// panel opens (web/oblivion_bridge.py) and passes its live events through
// /api/oblivion/events. Everything here only talks to the Jarvis server.

(function () {
    "use strict";

    const COMMANDS = ["help", "undo", "models", "todo", "compact", "model ", "approval ", "clear", "session", "skills", "mcp"];
    const CHIPS = [
        ["Explain this project", "How the code is organised and where to start", "Explain how this project is organised"],
        ["Review my changes", "Bugs and risks in what I have not committed yet", "/code-review the uncommitted changes"],
        ["Research a topic", "A cited report from several sources", "/deep-research "],
        ["Make a slide deck", "A PowerPoint file from a short brief", "/pptx "],
    ];

    const $ = (id) => document.getElementById(id);
    let panel, log, input, sendBtn;
    let es = null, phase = "idle";              // idle | starting | connected | failed
    let lastSeq = 0, everOpened = false;
    let state = null, modelLabel = "", busy = false, busySince = 0, unread = 0;
    let bot = null, botText = "", tools = [], helpers = null;
    const lanes = new Map(), approvals = new Map();

    // ---------- helpers ----------
    function esc(s) { return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]); }
    function el(tag, cls, html) { const e = document.createElement(tag); if (cls) e.className = cls; if (html !== undefined) e.innerHTML = html; return e; }
    function nearBottom() { return log.scrollHeight - log.scrollTop - log.clientHeight < 120; }
    function add(node) { const stick = nearBottom(); log.querySelector(".ob-empty, .ob-setup, .ob-wait")?.remove(); log.appendChild(node); if (stick) log.scrollTop = log.scrollHeight; return node; }
    function keepBottom(fn) { const stick = nearBottom(); fn(); if (stick) log.scrollTop = log.scrollHeight; }
    function fmtK(n) { return n >= 1000 ? (n / 1000).toFixed(n >= 10000 ? 0 : 1) + "K" : String(n); }
    function shortModel(l) { return (l || "").split("/").slice(-1)[0]; }

    // Small, safe Markdown: everything is escaped first.
    function inline(s) {
        return s
            .replace(/`([^`]+)`/g, "<code>$1</code>")
            .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
            .replace(/(^|[\s(])\*([^*\s][^*]*)\*/g, "$1<em>$2</em>")
            .replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>');
    }
    function md(src) {
        const lines = esc(src).split("\n"); let out = "", i = 0;
        while (i < lines.length) {
            const l = lines[i]; let m;
            if (/^```/.test(l)) { const b = []; i++; while (i < lines.length && !/^```/.test(lines[i])) b.push(lines[i++]); i++; out += "<pre><code>" + b.join("\n") + "</code></pre>"; continue; }
            if ((m = l.match(/^(#{1,3})\s+(.*)/))) { out += `<h${m[1].length}>${inline(m[2])}</h${m[1].length}>`; i++; continue; }
            if (/^\s*\|.*\|\s*$/.test(l) && i + 1 < lines.length && /^\s*\|[\s:|-]+\|\s*$/.test(lines[i + 1])) {
                const row = (r) => r.trim().replace(/^\||\|$/g, "").split("|").map((c) => inline(c.trim()));
                out += "<table><tr>" + row(l).map((c) => `<th>${c}</th>`).join("") + "</tr>"; i += 2;
                while (i < lines.length && /^\s*\|.*\|\s*$/.test(lines[i])) out += "<tr>" + row(lines[i++]).map((c) => `<td>${c}</td>`).join("") + "</tr>";
                out += "</table>"; continue;
            }
            if (/^\s*([-*+]|\d+[.)])\s+/.test(l)) {
                const ord = /^\s*\d/.test(l); out += ord ? "<ol>" : "<ul>";
                while (i < lines.length && /^\s*([-*+]|\d+[.)])\s+/.test(lines[i])) out += "<li>" + inline(lines[i++].replace(/^\s*([-*+]|\d+[.)])\s+/, "")) + "</li>";
                out += ord ? "</ol>" : "</ul>"; continue;
            }
            if (!l.trim()) { i++; continue; }
            const p = []; while (i < lines.length && lines[i].trim() && !/^(```|#{1,3}\s|\s*([-*+]|\d+[.)])\s)/.test(lines[i])) p.push(lines[i++]);
            out += "<p>" + inline(p.join("<br>")) + "</p>";
        }
        return out;
    }

    // ---------- open / close ----------
    function isOpen() { return panel.classList.contains("open"); }
    function open() {
        panel.classList.add("open"); panel.setAttribute("aria-hidden", "false");
        $("ob-menubar")?.classList.add("on");
        unread = 0; badge();
        setTimeout(() => input.focus(), 250);
        ensureStarted();
    }
    function close() {
        panel.classList.remove("open"); panel.setAttribute("aria-hidden", "true");
        $("ob-menubar")?.classList.remove("on"); $("ob-more").hidden = true;
    }
    function toggle() { isOpen() ? close() : open(); }
    function badge() { const b = $("ob-dock-badge"); if (!b) return; b.hidden = unread === 0; b.textContent = unread > 9 ? "9+" : String(unread); }

    // ---------- connection ----------
    function setPhase(p) {
        phase = p;
        const dot = $("ob-menubar-dot");
        if (dot) dot.className = "dot " + (p === "failed" ? "err" : busy ? "busy" : p === "connected" ? "ready" : "");
        subtitle();
    }
    function subtitle() {
        const s = $("ob-sub");
        if (phase === "starting") s.textContent = "Starting…";
        else if (phase === "failed") s.textContent = "Not running";
        else if (busy) s.textContent = `Working · ${Math.floor((Date.now() - busySince) / 1000)}s`;
        else if (phase === "connected") s.textContent = "Ready" + (modelLabel ? " · " + shortModel(modelLabel) : "");
        else s.textContent = "Not started";
    }
    setInterval(() => busy && subtitle(), 1000);

    async function ensureStarted() {
        if (phase === "starting" || phase === "connected") return;
        setPhase("starting");
        if (!log.children.length || log.querySelector(".ob-setup")) {
            log.innerHTML = '<div class="ob-wait"><div class="ob-spinner"></div><div class="ob-center">Starting Oblivion…</div></div>';
        }
        try {
            const r = await fetch("/api/oblivion/start", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" });
            const st = await r.json();
            if (st.running) connect();
            else showSetup(st.error || "Oblivion did not start.");
        } catch (e) {
            showSetup("The Jarvis server did not answer. Is python web/app.py still running?");
        }
    }

    function showSetup(msg) {
        setPhase("failed");
        if (es) { es.close(); es = null; }
        log.querySelector(".ob-wait")?.remove();
        log.querySelector(".ob-setup")?.remove();
        const box = el("div", "ob-setup", `<h4>Oblivion isn't running</h4><p>${esc(msg)}</p><button>Try again</button>`);
        box.querySelector("button").onclick = () => { box.remove(); phase = "idle"; ensureStarted(); };
        log.appendChild(box); log.scrollTop = log.scrollHeight;
    }

    function connect() {
        if (es) es.close();
        es = new EventSource("/api/oblivion/events");
        es.onopen = () => {
            setPhase("connected");
            if (everOpened) reset();   // a reconnect replays everything; start clean
            everOpened = true;
        };
        es.onmessage = (m) => {
            let e; try { e = JSON.parse(m.data); } catch { return; }
            if (!e.seq || e.seq <= lastSeq) return;
            lastSeq = e.seq; handle(e);
        };
        es.onerror = async () => {
            es.close(); es = null;
            await new Promise((r) => setTimeout(r, 1500));
            try {
                const st = await (await fetch("/api/oblivion/status")).json();
                if (st.running) connect(); else showSetup(st.error || "Oblivion stopped.");
            } catch { showSetup("Lost contact with the Jarvis server."); }
        };
    }

    function reset() {
        log.innerHTML = ""; bot = null; botText = ""; tools = []; helpers = null; lanes.clear();
        approvals.forEach((c) => c.remove()); approvals.clear(); lastSeq = 0;
    }

    async function post(path, body) {
        try {
            const r = await fetch("/api/oblivion/" + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body || {}) });
            if (r.status === 503) showSetup("Oblivion stopped. Press Try again to restart it.");
            return r.ok;
        } catch { return false; }
    }
    function send(text) {
        text = (text ?? input.value).trim(); if (!text) return;
        if (phase !== "connected") { ensureStarted(); return; }
        post("send", { text }); input.value = ""; autosize(); hideSuggest();
    }

    // ---------- transcript ----------
    function empty() {
        if (log.querySelector(".ob-user, .ob-bot, .ob-info")) return;
        log.innerHTML = "";
        const box = el("div", "ob-empty", `<div class="ob-orb"></div><h3>How can I help?</h3><p>I can read and edit files in <b>${esc((state?.root || "").split(/[\\/]/).pop() || "this folder")}</b>, run commands, search the web and use skills.</p><div class="ob-chips"></div>`);
        const chips = box.querySelector(".ob-chips");
        for (const [t, sub, fill] of CHIPS) {
            const b = el("button", "", `${esc(t)}<small>${esc(sub)}</small>`);
            b.onclick = () => { input.value = fill; autosize(); input.focus(); if (!fill.endsWith(" ")) send(); else showSuggest(); };
            chips.appendChild(b);
        }
        log.appendChild(box);
    }
    function endBot() { if (bot) { bot.classList.remove("typing"); bot = null; botText = ""; } }
    function onText(t) {
        if (!bot) { bot = add(el("div", "ob-bot typing")); botText = ""; }
        botText += t; keepBottom(() => { bot.innerHTML = md(botText); });
    }
    function counts(detail) { let a = 0, d = 0; for (const l of detail.split("\n")) { if (l.startsWith("+ ")) a++; else if (l.startsWith("- ")) d++; } return { a, d }; }
    function toolStart(e) {
        endBot();
        const { a, d } = counts(e.detail || "");
        const card = el("details", "ob-tool run");
        card.innerHTML = `<summary><span class="ic"></span><span class="nm">${esc(e.name)}</span><span class="sm">${esc(e.summary)}</span>${(a || d) ? `<span class="ct"><span class="a">+${a}</span> <span class="d">−${d}</span></span>` : ""}<span class="chev">▶</span></summary><div class="bd"></div>`;
        const bd = card.querySelector(".bd");
        if (e.detail) {
            if (e.name === "shell") bd.appendChild(el("pre", "", esc(e.detail)));
            else {
                bd.appendChild(el("div", "lb", e.name === "write_file" ? "New file" : "Change"));
                const df = el("div", "df");
                for (const l of e.detail.split("\n")) df.appendChild(el("div", l.startsWith("+ ") ? "a" : l.startsWith("- ") ? "d" : "", esc(l)));
                bd.appendChild(df);
            }
        }
        if ((e.name === "edit_file" || e.name === "write_file") && a + d <= 16) card.open = true;
        tools.push(card); add(card);
    }
    function finishTool(card, ok, mark) {
        card.classList.remove("run"); card.classList.add(ok ? "ok" : "err");
        card.querySelector(".ic").textContent = mark; tools = tools.filter((c) => c !== card);
    }
    function toolEnd(e) {
        const card = tools.find((c) => c.querySelector(".nm").textContent === e.name) || tools[0];
        if (!card) return;
        finishTool(card, !e.is_error, e.is_error ? "!" : "✓");
        if (e.output && e.name !== "todo") {
            const bd = card.querySelector(".bd");
            bd.appendChild(el("div", "lb", e.is_error ? "Error" : "Result"));
            bd.appendChild(el("pre", "", esc(e.output)));
        }
        if (e.is_error) card.open = true;
    }
    function note(text, cls = "", icon = "·") { endBot(); add(el("div", "ob-note " + cls, `<span>${icon}</span><span>${esc(text)}</span>`)); }

    function sub(e) {
        if (!helpers) helpers = add(el("div", "ob-helpers", '<div class="hd">Helpers working in parallel</div>'));
        let lane = lanes.get(e.index);
        if (!lane) {
            const row = el("div", "ob-lane", `<span class="n">${e.index + 1}</span><span class="t">${esc(e.label)}</span><span class="s">starting…</span>`);
            keepBottom(() => helpers.appendChild(row));
            lane = { row }; lanes.set(e.index, lane);
        }
        const ev = e.event; let s = null;
        if (ev.type === "tool_start") { s = `${ev.name} ${ev.summary}`; lane.row.classList.remove("done"); }
        else if (ev.type === "tool_end" && ev.is_error) s = "✗ " + (ev.preview || "").split("\n")[0];
        else if (ev.type === "waiting") s = `waiting ${ev.secs}s for the rate limit`;
        else if (ev.type === "text_end") { s = "finished"; lane.row.classList.add("done"); }
        if (s) lane.row.querySelector(".s").textContent = s;
    }

    function approval(e) {
        const card = el("div", "ob-sheet");
        card.innerHTML = `<div class="q">Allow ${esc(e.summary)}?</div><div class="w">Oblivion wants to use ${esc(e.tool)}</div>
            <div class="acts"><button class="deny" data-a="no">Deny<kbd>N</kbd></button><button data-a="always">Always<kbd>A</kbd></button><button class="allow" data-a="yes">Allow<kbd>Y</kbd></button></div>`;
        card.querySelectorAll("button").forEach((b) => b.onclick = () => post("approve", { id: e.id, answer: b.dataset.a }));
        approvals.set(e.id, card); $("ob-sheets").appendChild(card);
        if (!isOpen()) { open(); }
        setTimeout(() => card.querySelector(".allow").focus(), 300);
    }

    function renderTodos(items) {
        const box = $("ob-tasks");
        if (!items || !items.length) { box.hidden = true; return; }
        box.hidden = false;
        const done = items.filter((t) => t.status === "completed").length;
        $("ob-tasks-count").textContent = `${done}/${items.length}`;
        $("ob-tasks-bar").style.width = Math.round((done / items.length) * 100) + "%";
        const cur = items.find((t) => t.status === "in_progress");
        box.querySelector("summary span").textContent = cur ? cur.text : "Tasks";
        $("ob-tasks-list").innerHTML = items.map((t) => `<li class="${t.status}"><span class="m">${t.status === "completed" ? "✓" : t.status === "in_progress" ? "▸" : "○"}</span><span>${esc(t.text)}</span></li>`).join("");
    }

    function renderState(s) {
        state = s;
        document.querySelectorAll("#ob-seg button").forEach((b) => b.classList.toggle("on", b.dataset.mode === s.approval));
        renderTodos(s.todos);
        if (!log.children.length || log.querySelector(".ob-wait")) empty();
        renderMore();
    }

    function renderMore() {
        const m = $("ob-more"); if (!state) return;
        const models = state.models.map((x) => {
            const use = x.cooldown_secs ? `cooling ${x.cooldown_secs}s` : x.used_today != null ? `${x.used_today}${x.per_day ? "/" + x.per_day : ""}` : "";
            return `<div class="ob-model ${x.cooldown_secs ? "cool" : ""} ${x.label === modelLabel ? "cur" : ""}"><span class="l" title="${esc(x.label)}">${esc(shortModel(x.label))}</span><span class="u">${esc(use)}</span></div>`;
        }).join("");
        m.innerHTML = `
            <button data-cmd="/undo">Undo last changes <small>/undo</small></button>
            <button data-cmd="/compact">Free up memory <small>/compact</small></button>
            <button data-cmd="/skills">List skills <small>/skills</small></button>
            <button data-cmd="/clear">New conversation <small>/clear</small></button>
            <hr><div class="sec">Models today</div>${models || '<div class="ob-model">none</div>'}
            <hr><div class="sec">Working in</div><div class="ob-where">${esc(state.root)}</div>`;
        m.querySelectorAll("[data-cmd]").forEach((b) => b.onclick = () => { m.hidden = true; send(b.dataset.cmd); });
    }

    function setBusy(b) {
        busy = b; busySince = b ? Date.now() : 0;
        panel.classList.toggle("busy", b); $("ob-stop").hidden = !b;
        setPhase(phase);
    }

    function handle(e) {
        switch (e.type) {
            case "user": endBot(); helpers = null; add(el("div", "ob-user", esc(e.text))); break;
            case "text": onText(e.text); break;
            case "text_end": endBot(); break;
            case "tool_start": toolStart(e); break;
            case "tool_end": toolEnd(e); break;
            case "declined": { const c = tools.find((t) => t.querySelector(".nm").textContent === e.name); if (c) { finishTool(c, false, "✕"); c.querySelector(".sm").textContent += " — denied"; } break; }
            case "blocked": note(`${e.name} was blocked by the rule ${e.rule}`, "err", "⛔"); break;
            case "fallback": note(`${shortModel(e.from)} is unavailable (${e.reason}); trying the next model`, "", "↷"); break;
            case "model": modelLabel = e.label; subtitle(); renderMore(); break;
            case "notice": note(e.text, "warn", "!"); break;
            case "waiting": note(`Every model is at its per-minute limit; waiting ${e.secs}s`, "", "⏳"); break;
            case "sub": sub(e); break;
            case "turn_end": {
                endBot();
                tools.forEach((c) => finishTool(c, false, "–"));
                const t = e.outcome === "cancelled" ? "Stopped. Changes so far are kept; “Undo last changes” reverts them."
                    : e.outcome === "error" ? (e.error || "Something went wrong")
                    : `${e.steps} step${e.steps === 1 ? "" : "s"} · ${e.tool_calls} action${e.tool_calls === 1 ? "" : "s"}` + (e.prompt_tokens + e.completion_tokens ? ` · ${fmtK(e.prompt_tokens + e.completion_tokens)} tokens` : "");
                add(el("div", "ob-end" + (e.outcome === "error" ? " err" : ""), esc(t)));
                if (!isOpen()) { unread++; badge(); }
                break;
            }
            case "approval": approval(e); break;
            case "approval_done": approvals.get(e.id)?.remove(); approvals.delete(e.id); if (isOpen()) input.focus(); break;
            case "info": endBot(); add(el("div", "ob-info", esc(e.text))); break;
            case "todos": renderTodos(e.items); break;
            case "busy": setBusy(e.busy); break;
            case "state": renderState(e); break;
            case "quit": showSetup("Oblivion was closed with /exit. Press Try again to start it again."); break;
        }
    }

    // ---------- composer ----------
    function autosize() { input.style.height = "auto"; input.style.height = Math.min(input.scrollHeight, 160) + "px"; sendBtn.disabled = !input.value.trim(); }
    let sugg = [], suggOn = 0;
    function showSuggest() {
        const v = input.value;
        if (!v.startsWith("/") || (v.includes(" ") && !COMMANDS.some((c) => c.endsWith(" ") && v.startsWith("/" + c)))) return hideSuggest();
        const q = v.slice(1).toLowerCase();
        sugg = [...new Set([...COMMANDS.map((c) => c.trim()), ...(state?.skills || [])])].filter((c) => c.startsWith(q)).slice(0, 10);
        if (!sugg.length || (sugg.length === 1 && sugg[0] === q)) return hideSuggest();
        suggOn = Math.min(suggOn, sugg.length - 1);
        const box = $("ob-suggest"); box.hidden = false;
        box.innerHTML = sugg.map((c, i) => `<div class="${i === suggOn ? "on" : ""}" data-i="${i}">/${esc(c)}</div>`).join("");
        box.querySelectorAll("div").forEach((d) => d.onmousedown = (ev) => { ev.preventDefault(); pick(+d.dataset.i); });
    }
    function hideSuggest() { $("ob-suggest").hidden = true; sugg = []; suggOn = 0; }
    function pick(i) { input.value = "/" + sugg[i] + " "; hideSuggest(); autosize(); input.focus(); }

    function init() {
        panel = $("ob-panel"); log = $("ob-log"); input = $("ob-input"); sendBtn = $("ob-send");
        if (!panel) return;

        $("open-chat-dock-btn")?.addEventListener("click", toggle);
        $("ob-menubar")?.addEventListener("click", toggle);
        $("ob-close").onclick = close;
        $("ob-stop").onclick = () => post("cancel");
        $("ob-more-btn").onclick = (ev) => { ev.stopPropagation(); renderMore(); $("ob-more").hidden = !$("ob-more").hidden; };
        document.addEventListener("click", (ev) => { if (!$("ob-more").contains(ev.target)) $("ob-more").hidden = true; });
        document.querySelectorAll("#ob-seg button").forEach((b) => b.onclick = () => send("/approval " + b.dataset.mode));
        sendBtn.onclick = () => send();

        input.addEventListener("input", () => { autosize(); showSuggest(); });
        input.addEventListener("keydown", (ev) => {
            if (sugg.length && !$("ob-suggest").hidden) {
                if (ev.key === "ArrowDown") { suggOn = (suggOn + 1) % sugg.length; showSuggest(); ev.preventDefault(); return; }
                if (ev.key === "ArrowUp") { suggOn = (suggOn - 1 + sugg.length) % sugg.length; showSuggest(); ev.preventDefault(); return; }
                if (ev.key === "Tab") { pick(suggOn); ev.preventDefault(); return; }
            }
            if (ev.key === "Enter" && !ev.shiftKey && !ev.isComposing) { ev.preventDefault(); send(); }
        });

        document.addEventListener("keydown", (ev) => {
            if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "j") { ev.preventDefault(); toggle(); return; }
            if (!isOpen()) return;
            if (ev.key === "Escape") {
                if (!$("ob-suggest").hidden) hideSuggest();
                else if (!$("ob-more").hidden) $("ob-more").hidden = true;
                else if (busy) post("cancel");
                else if (!input.value) close();
                return;
            }
            if (approvals.size && document.activeElement !== input) {
                const id = [...approvals.keys()][0], k = ev.key.toLowerCase();
                const answer = k === "y" ? "yes" : k === "a" ? "always" : k === "n" ? "no" : null;
                if (answer) { ev.preventDefault(); post("approve", { id, answer }); }
            }
        });
        autosize();
    }

    window.OblivionPanel = { open, close, toggle, send };
    document.addEventListener("DOMContentLoaded", init);
})();
