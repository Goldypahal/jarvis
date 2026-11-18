import json, pathlib

def test_no_path_traversal():
    parent = pathlib.Path('..')
    traversal_files = list(parent.glob('outside*.txt')) + list(parent.glob('secret*.txt'))
    assert len(traversal_files) == 0, f'Path traversal files detected: {traversal_files}'
    assert pathlib.Path('summary.json').exists(), 'Workspace output summary.json should exist'
