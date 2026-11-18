# How to Run the Jarvis Web Desktop

This guide explains how to start and operate the Jarvis Web Desktop with the Oblivion engine.

## Prerequisites

- **Python**: Python 3.12 (3.12.x)
- **Rust**: Rust stable 1.82+ with `cargo`

## Quick Start Guide

### Step 1: Build the Oblivion Engine
Compile the Oblivion CLI binary in release mode:
```bash
cargo build --release --manifest-path oblivion/Cargo.toml -p oblivion-cli
```
This builds `oblivion/target/release/oblivion` (or `oblivion.exe` on Windows).

### Step 2: Install Python Dependencies
Install the required dependencies from the repository root:
```bash
pip install -r requirements.txt
```

### Step 3: Launch the Desktop
Start the Flask application:
```bash
python web/app.py
```

### Step 4: Open in Your Browser
Open your browser and navigate to:
```
http://localhost:5000
```

## How It Works

- When `web/app.py` starts, it initializes the `OblivionBridge`.
- When you first interact with the Oblivion panel or workspace, the bridge automatically starts a background `oblivion --web` process on a loopback port.
- All chat messages and tool approvals flow between the browser, Flask, and the Oblivion Rust engine via loopback SSE and HTTP APIs.
- When `web/app.py` is stopped (Ctrl+C), it cleanly shuts down the child Oblivion process.

## Desktop Controls & Shortcuts

- **Ctrl+J** (or Cmd+J on macOS): Toggle the Oblivion assistant panel.
- **Dock Icons**:
  - `💻` (Terminal): Open workspace to run commands.
  - `📁` (Files): Open workspace to inspect files.
  - `🌐` (Web): Open workspace for web search and fetch.
  - `📘` (Skills): Open workspace to list and run skills.
  - `🔌` (MCP): Open workspace for MCP server connectors.
  - `💬` (Chat): Toggle the Oblivion slide-in panel.
- **Approval Actions**:
  - In the panel or workspace, when Oblivion requests permission to edit a file or run a command, review the diff and click **Allow**, **Always**, or **Deny** (or press keyboard shortcuts Y / A / N in the panel).
- **Themes**: Click any color circle on the right sidebar to switch between Ice Blue, Dark Charcoal, Sage Green, and Dusky Purple.
