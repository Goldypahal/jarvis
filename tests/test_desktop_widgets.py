"""Tests for real desktop widgets and live date (B7).

Verifies that:
1. Hardcoded date ("Aug 12") is replaced with live date container (#menu-date).
2. Fake music card ("Now Playing", "soundcheck") is completely removed.
3. Real widgets (usage today, tasks, files created, memory count, model status) are present in the DOM.
4. /api/oblivion/dashboard returns valid real statistics.
5. Desktop icons open real files/collections instead of fake "Fetching assets...".
"""
import pytest
from web.app import app


@pytest.fixture
def client():
    app.config['TESTING'] = True
    with app.test_client() as client:
        yield client


def test_no_hardcoded_date_and_live_date_container_present(client):
    res = client.get('/')
    assert res.status_code == 200
    html = res.get_data(as_text=True)

    # Fixed date text "Aug 12" must not exist
    assert 'Aug 12' not in html
    # Live date element must exist
    assert 'id="menu-date"' in html or 'class="menu-date"' in html
    assert 'id="menu-date"' in html


def test_fake_music_card_removed(client):
    res = client.get('/')
    assert res.status_code == 200
    html = res.get_data(as_text=True)

    # Fake music card elements must be gone
    assert 'NOW PLAYING' not in html
    assert 'soundcheck' not in html
    assert 'Jarvis Soundscapes' not in html
    assert 'open in apple music' not in html


def test_real_widgets_present_in_html(client):
    res = client.get('/')
    assert res.status_code == 200
    html = res.get_data(as_text=True)

    # 1. Usage today widget
    assert 'id="usage-widget"' in html or 'usage-widget' in html
    assert 'USAGE TODAY' in html

    # 2. Current tasks widget
    assert 'id="tasks-widget"' in html or 'tasks-widget' in html
    assert 'CURRENT TASKS' in html

    # 3. Files created this week widget
    assert 'id="files-created-widget"' in html or 'files-created-widget' in html
    assert 'FILES CREATED' in html

    # 4. Long-term memory count widget
    assert 'id="memory-widget"' in html or 'memory-widget' in html
    assert 'LONG-TERM MEMORY' in html

    # 5. Engine & model status widget
    assert 'id="engine-status-widget"' in html or 'engine-status-widget' in html
    assert 'ENGINE &amp; STATUS' in html or 'ENGINE & STATUS' in html


def test_desktop_icons_and_files_modal_present(client):
    res = client.get('/')
    assert res.status_code == 200
    html = res.get_data(as_text=True)

    # Files modal for real assets / files created
    assert 'id="filesModal"' in html
    # Desktop icons exist
    assert 'Research Papers' in html
    assert 'Pictures Uploaded &amp; Generated' in html or 'Pictures Uploaded & Generated' in html
    assert 'Documents Shared' in html
    assert 'Notebook' in html
    assert 'Case Studies' in html


def test_api_dashboard_endpoint(client):
    # Without token, request must be rejected with 403 (S6 security)
    res_unauth = client.get('/api/oblivion/dashboard')
    assert res_unauth.status_code == 403

    # With token, request succeeds
    client.environ_base["HTTP_X_JARVIS_TOKEN"] = app.config["JARVIS_TOKEN"]
    res = client.get('/api/oblivion/dashboard')
    assert res.status_code == 200
    data = res.get_json()
    assert 'usage_today' in data
    assert 'tasks' in data
    assert 'recent_files' in data
    assert 'memories_count' in data
    assert 'engine' in data
    assert 'version' in data['engine']
    assert 'workspace' in data['engine']
