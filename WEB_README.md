# NEXUS Web User Interface

This web interface provides a modern, dark-themed UI for interacting with the NEXUS AI assistant, inspired by the portfolio site at https://www.parinazkassemi.com/

## Features

- **Dark Theme UI**: Modern interface with accent colors and clean layout
- **Chat Interface**: Real-time communication with NEXUS
- **Mode Controls**: Activate/deactivate specialized modes (Deep Focus, Learning Companion, etc.)
- **Skills Panel**: View and manage learned skills
- **Responsive Design**: Works on desktop and mobile devices
- **Typing Indicators**: Visual feedback when NEXUS is processing
- **Skill Learning Integration**: Full access to the skill learning system

## UI Components

### Header
- Displays NEXUS title and subtitle with gradient text effect

### Sidebar
- **Modes Section**: Buttons to activate specialized modes
- **Skills Section**: Lists all learned skills with proficiency levels
- **Settings Section**: Placeholder for future settings

### Chat Area
- Message display area with user and NEXUS messages styled differently
- Input field for sending messages to NEXUS
- Real-time typing indicators when NEXUS is processing

## Installation

1. Install the required dependency:
   ```
   pip install -r web/requirements.txt
   ```

2. Run the web application:
   ```
   python web/app.py
   ```

3. Open your web browser and navigate to:
   ```
   http://localhost:5000
   ```

## API Endpoints

The web interface communicates with the NEXUS backend through these endpoints:

- `POST /api/chat` - Send a message to NEXUS and get a response
- `GET /api/skills` - Retrieve list of learned skills
- `GET /api/status` - Get current system status

## How It Works

The web interface consists of:

1. **Frontend** (in `web/` directory):
   - `templates/index.html` - Main HTML structure
   - `static/css/style.css` - Dark theme styling
   - `static/js/app.js` - UI logic and API communication

2. **Backend** (in `web/` directory):
   - `app.py` - Flask application serving the UI and handling API requests
   - `nexus.py` - Adapter wrapping existing NEXUS core components

## Dependencies

- Flask 2.3.3

## Notes

- The web interface uses the same RAG path as specified in your requirements: `D:\Desktop\RAG`
- Memory is stored in `nexus_web_memory.db` (separate from the console version)
- All existing NEXUS functionality is preserved and accessible through the web UI
- The UI is designed to be intuitive and follows modern web design principles

Enjoy interacting with NEXUS through this modern web interface!