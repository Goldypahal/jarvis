from app import greet
import config

def test_greet():
    assert greet() == 'Hello, friend!'
    assert greet('Ana') == 'Hello, Ana!'
    assert config.DEFAULTS['name'] == 'friend'
