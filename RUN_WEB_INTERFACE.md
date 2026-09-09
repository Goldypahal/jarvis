# How to Run the NEXUS Web Interface

## Prerequisites
1. Python 3.14.0+ installed (as specified in CLAUDE.md)
2. The NEXUS core system must be installed and available in the Jarvis directory
3. Flask web framework

## Installation Steps

### 1. Install Dependencies
```bash
cd C:\Users\Asus\free-claude-code\Jarvis\web
pip install -r requirements.txt
```

### 2. Verify NEXUS Core is Available
Make sure the NEXUS core components are accessible from the web directory. The nexus.py adapter expects to find the NEXUS modules in the parent directory (..\nexus\\).

### 3. Start the Web Server
```bash
cd C:\Users\Asus\free-claude-code\Jarvis\web
python app.py
```

### 4. Access the Interface
Open your web browser and navigate to:
```
http://localhost:5000
```

## Features Available

### Chat Interface
- Send messages to NEXUS and receive responses
- See typing indicators when NEXUS is processing
- Conversation history preserved in the chat window
- Responsive design works on mobile and desktop

### Mode Controls (Sidebar)
- **Deep Focus**: For concentrated, uninterrupted work
- **Learning Companion**: For acquiring new knowledge or skills
- **Creative Collaboration**: For brainstorming and idea generation
- **Strategic Planning**: For long-term planning and decision making
- Visual indication shows which mode is active

### Skills Panel (Sidebar)
- Lists all skills learned through the skill learning system
- Shows skill name, type, level, and proficiency percentage
- Refresh button to update the skills list
- Integration with the skill learning system implemented earlier

## Using the Skill Learning System via Web UI
All skill learning commands work through the chat interface:
- `skill learn Python programming` - Learn a new skill
- `skill language Spanish` - Learn a new language
- `skill app chrome` - Learn to control an application
- `skill list` - View all learned skills
- `skill info <skill_id>` - Get detailed skill information
- `skill use <skill_id> <action>` - Use a learned skill
- `skill practice <skill_id> [minutes]` - Practice to improve proficiency

## Design Notes
The UI is designed to resemble the aesthetic of https://www.parinazkassemi.com/ with:
- Dark color scheme with blue accent colors (#00d4ff)
- Clean, modern typography
- Responsive layout
- Subtle animations and hover effects
- Intuitive navigation and controls

## Troubleshooting
If you encounter issues:
1. Make sure you're running from the `web` directory
2. Check that all NEXUS core modules are accessible
3. Verify Python 3.14.0+ is being used
4. Check the console for any error messages
5. Ensure port 5000 is available (not being used by another application)

## Notes
- The web interface uses a separate memory database (`nexus_web_memory.db`) to avoid conflicts with the console version
- All NEXUS core functionality (reasoning, memory, context, etc.) is preserved
- The interface can run simultaneously with the console version if desired
- The RAG system path is set to `D:\Desktop\RAG` as specified in your requirements