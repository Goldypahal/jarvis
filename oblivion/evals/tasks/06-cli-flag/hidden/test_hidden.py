import subprocess, sys

def run(*a):
    return subprocess.run([sys.executable, 'shout.py', *a], capture_output=True, text=True).stdout.strip()

def test_flag():
    assert run('hi there') == 'hi there'
    assert run('hi there', '--upper') == 'HI THERE'
