# Jarvis Web Desktop

The Jarvis Web Desktop is a modern executive desktop interface that hosts the **Oblivion** autonomous agent.

## Features

- **Executive Desktop Canvas**: Themeable workspace (Ice Blue, Dark Charcoal, Sage Green, Dusky Purple) with quick-access nodes and customizable capability icons.
- **30/70 Workspace Split**:
  - Left 30%: Interactive chat space communicating directly with the Oblivion agent.
  - Right 70%: Live execution workspace showing active task breakdowns, tool execution cards with diffs, and pending user approvals.
- **Oblivion Slide-in Panel (Ctrl+J)**: Collapsible agent panel with streaming responses, task progress, model selection, and segmented approval controls (Ask / Auto-edit / Never ask).
- **Security-First Design**:
  - Bound to `127.0.0.1` by default with strict `Host` header enforcement.
  - Token-authenticated `/api/oblivion/*` endpoints to prevent cross-origin drive-by attacks.
  - Bundled local fonts and opt-in weather widget (zero unprompted remote requests).

## Quick Start

1. **Build Oblivion**:
   ```bash
   cargo build --release --manifest-path oblivion/Cargo.toml -p oblivion-cli
   ```

2. **Install Python Requirements**:
   ```bash
   pip install -r requirements.txt
   ```

3. **Start the Web Desktop**:
   ```bash
   python web/app.py
   ```

4. Navigate to `http://localhost:5000` in your browser.

## Architecture

- **`web/app.py`**: Flask server hosting the web templates, static assets, and the `/api/chat` route.
- **`web/oblivion_bridge.py`**: Manages the child `oblivion --web` process, proxies API requests to it, and provides Server-Sent Events (SSE) streaming at `/api/oblivion/events`.
- **`web/static/js/oblivion.js`**: Client-side state manager and event handler for the Oblivion panel and 30/70 workspace view.
- **`web/static/js/app.js`**: Desktop canvas controller (clock, themes, desktop icon actions, modal transitions).

## Testing

Run the web test suite:
```bash
python -m pytest -q tests
node --check web/static/js/app.js
node --check web/static/js/oblivion.js
```
