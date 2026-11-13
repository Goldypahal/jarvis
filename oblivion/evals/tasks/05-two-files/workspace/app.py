from config import DEFAULTS

def greet(name=None):
    return f"{DEFAULTS['greeting']}, {name or DEFAULTS['name']}!"
