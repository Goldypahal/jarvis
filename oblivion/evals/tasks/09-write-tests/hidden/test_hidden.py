import ast, pathlib, subprocess, sys

def test_written_tests():
    src = pathlib.Path('test_stack.py').read_text()
    tests = [n for n in ast.walk(ast.parse(src)) if isinstance(n, ast.FunctionDef) and n.name.startswith('test')]
    assert len(tests) >= 4
    r = subprocess.run([sys.executable, '-m', 'pytest', '-q', 'test_stack.py'], capture_output=True, text=True)
    assert r.returncode == 0, r.stdout
