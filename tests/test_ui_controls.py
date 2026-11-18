"""Tests for UI controls in web/templates/index.html to ensure no dead NEXUS controls remain."""
import pytest
from web.app import app


@pytest.fixture
def client():
    app.config['TESTING'] = True
    with app.test_client() as client:
        yield client


def test_index_page_loads_and_has_no_fake_nexus_controls(client):
    response = client.get('/')
    assert response.status_code == 200
    html = response.get_data(as_text=True)

    # Must not contain NEXUS brand or fake gamification
    assert 'NEXUS' not in html
    assert '231' not in html  # Fake streak
    assert 'DAY STREAK' not in html
    assert 'System 1/2' not in html
    assert 'dual-process' not in html
    assert 'RAG skill learning' not in html
    assert 'Skill Learning Lab' not in html

    # Must contain real Oblivion desktop elements
    assert 'Jarvis' in html
    assert 'Oblivion' in html
    assert 'Oblivion v0.5' in html
    assert 'Sandboxed Tools' in html


def test_no_dead_routes_in_html(client):
    response = client.get('/')
    html = response.get_data(as_text=True)

    dead_routes = [
        '/api/skills',
        '/api/status',
        '/api/modes',
        '/api/tasks/decompose',
    ]
    for route in dead_routes:
        assert route not in html
