"""
Jarvis Web Desktop
A Flask application that provides the desktop interface for the Oblivion agent.
"""

import os
import sys
from flask import Flask, render_template, request, jsonify

# Add the current directory and project root to the path
current_dir = os.path.dirname(os.path.abspath(__file__))
project_root = os.path.abspath(os.path.join(current_dir, '..'))

if current_dir not in sys.path:
    sys.path.insert(0, current_dir)
if project_root not in sys.path:
    sys.path.insert(1, project_root)

import oblivion_bridge  # Runs the Oblivion agent bridge

app = Flask(__name__, template_folder='templates', static_folder='static')
jarvis_port = int(os.environ.get('JARVIS_PORT', '5000'))
app.config['JARVIS_PORT'] = jarvis_port
oblivion_bridge.register(app, allowed_port=jarvis_port)


@app.route('/')
def index():
    """Serve the main UI page."""
    return render_template('index.html', jarvis_token=app.config.get('JARVIS_TOKEN', ''))


@app.route('/api/chat', methods=['POST'])
def chat():
    """Handle chat messages from the UI (forwards to Oblivion through the bridge)."""
    try:
        data = request.get_json(silent=True) or {}
        message = data.get('message', '')
        if not message or not str(message).strip():
            return jsonify({'response': 'Please enter a message.'}), 400

        bridge = app.config.get('OBLIVION_BRIDGE')
        if not bridge:
            return jsonify({'response': 'Oblivion bridge is not configured.'}), 500

        result = bridge.send_and_wait(message)
        return jsonify(result)
    except TimeoutError:
        return jsonify({'response': 'Oblivion request timed out.'}), 504
    except Exception as e:
        print(f"Error in chat endpoint: {e}")
        return jsonify({'response': 'Sorry, I encountered an error. Please try again.'}), 500


if __name__ == '__main__':
    # Only this computer can open the desktop: the Oblivion panel can edit files and
    # run commands, so it must not be reachable from the network. Debug mode stays
    # off because its in-browser console would let a visitor run code.
    # Set JARVIS_HOST=0.0.0.0 only if you understand that risk.
    host = os.environ.get('JARVIS_HOST', '127.0.0.1')
    port = int(os.environ.get('JARVIS_PORT', '5000'))
    print(f"Jarvis desktop: http://{'localhost' if host == '127.0.0.1' else host}:{port}")
    app.run(host=host, port=port, debug=False, threaded=True)
