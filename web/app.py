"""
NEXUS Web Interface
A Flask web application that provides a modern UI for interacting with NEXUS
"""

from flask import Flask, render_template, request, jsonify
import asyncio
import sys
import os

# Add the current directory to the path so we can import our local adapter module
sys.path.insert(0, os.path.dirname(__file__))

from nexus_adapter import NexusCore  # Our local adapter

app = Flask(__name__, template_folder='templates', static_folder='static')

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
    # Run the Flask app
    app.run(host='0.0.0.0', port=5000, debug=True)