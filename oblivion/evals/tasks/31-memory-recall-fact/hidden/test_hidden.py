import pathlib

def test_recalled_port():
    f = pathlib.Path("port.txt")
    assert f.exists(), "port.txt was not created"
    content = f.read_text(encoding="utf-8").strip()
    assert "9443" in content, f"Expected port 9443 in port.txt, got: {content}"
