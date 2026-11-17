"""
NEXUS Web Interface
A Flask web application that provides a modern UI for interacting with NEXUS
"""

from flask import Flask, render_template, request, jsonify
import asyncio
import sys
import os

# Add the current directory and project root to the path so we can import our local adapter and core modules
current_dir = os.path.dirname(os.path.abspath(__file__))
project_root = os.path.abspath(os.path.join(current_dir, '..'))

if current_dir not in sys.path:
    sys.path.insert(0, current_dir)
if project_root not in sys.path:
    sys.path.insert(1, project_root)

from nexus_adapter import NexusCore  # Our local adapter
import oblivion_bridge  # Runs the Oblivion agent for the right-side panel

app = Flask(__name__, template_folder='templates', static_folder='static')
jarvis_port = int(os.environ.get('JARVIS_PORT', '5000'))
app.config['JARVIS_PORT'] = jarvis_port
oblivion_bridge.register(app, allowed_port=jarvis_port)

# Initialize NEXUS core (we'll create a singleton instance)
nexus_core = None

def get_nexus_core():
    global nexus_core
    if nexus_core is None:
        nexus_core = NexusCore()
    return nexus_core

def run_async(coro):
    return asyncio.run(coro)

@app.route('/')
def index():
    """Serve the main UI page"""
    return render_template('index.html')

@app.route('/api/chat', methods=['POST'])
def chat():
    """Handle chat messages from the UI"""
    try:
        data = request.get_json()
        message = data.get('message', '')
        mode = data.get('mode', None)

        nexus = get_nexus_core()
        result = run_async(nexus.process_query(message, mode=mode))

        return jsonify({
            'response': result.get('response', 'I apologize, but I encountered an error processing your request.')
        })
    except Exception as e:
        print(f"Error in chat endpoint: {e}")
        return jsonify({'response': 'Sorry, I encountered an error. Please try again.'}), 500

@app.route('/api/skills', methods=['GET'])
def get_skills():
    """Get list of learned skills"""
    try:
        nexus = get_nexus_core()
        skills = run_async(nexus.get_skills())
        return jsonify(skills)
    except Exception as e:
        print(f"Error in skills endpoint: {e}")
        return jsonify([]), 500

@app.route('/api/status', methods=['GET'])
def get_status():
    """Get current system status"""
    try:
        nexus = get_nexus_core()
        status = run_async(nexus.get_status())
        return jsonify(status)
    except Exception as e:
        print(f"Error in status endpoint: {e}")
        return jsonify({'error': 'Unable to get status'}), 500

@app.route('/api/skills/learn', methods=['POST'])
def learn_skill():
    """Learn a new skill"""
    try:
        data = request.get_json()
        name = data.get('name', 'New Skill')
        description = data.get('description', '')
        skill_type = data.get('skill_type', 'coding')

        nexus = get_nexus_core()
        res = run_async(nexus.learn_skill(name, description, skill_type))
        return jsonify(res)
    except Exception as e:
        print(f"Error in learn skill endpoint: {e}")
        return jsonify({'error': str(e)}), 500

@app.route('/api/tasks/decompose', methods=['POST'])
def decompose_goal():
    """Decompose a high-level goal into tasks"""
    try:
        data = request.get_json()
        goal = data.get('goal', '')
        timeframe = data.get('timeframe', '1 week')

        nexus = get_nexus_core()
        res = run_async(nexus.decompose_goal(goal, timeframe))
        return jsonify(res)
    except Exception as e:
        print(f"Error in task decomposition endpoint: {e}")
        return jsonify({'error': str(e)}), 500

@app.route('/api/modes/activate', methods=['POST'])
def activate_mode():
    """Activate a specialized mode"""
    try:
        data = request.get_json()
        mode = data.get('mode', '')
        nexus = get_nexus_core()
        res = run_async(nexus.activate_mode(mode))
        return jsonify({'message': res, 'active_mode': mode})
    except Exception as e:
        print(f"Error activating mode: {e}")
        return jsonify({'error': str(e)}), 500

if __name__ == '__main__':
    # Only this computer can open the desktop: the Oblivion panel can edit files and
    # run commands, so it must not be reachable from the network. Debug mode stays
    # off because its in-browser console would let a visitor run code.
    # Set JARVIS_HOST=0.0.0.0 only if you understand that risk.
    host = os.environ.get('JARVIS_HOST', '127.0.0.1')
    port = int(os.environ.get('JARVIS_PORT', '5000'))
    print(f"Jarvis desktop: http://{'localhost' if host == '127.0.0.1' else host}:{port}")
    app.run(host=host, port=port, debug=False, threaded=True)