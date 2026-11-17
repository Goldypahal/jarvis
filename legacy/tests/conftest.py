import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
LEGACY_DIR = os.path.abspath(os.path.join(HERE, ".."))
REPO_ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))

if LEGACY_DIR not in sys.path:
    sys.path.insert(0, LEGACY_DIR)
if REPO_ROOT not in sys.path:
    sys.path.insert(1, REPO_ROOT)
