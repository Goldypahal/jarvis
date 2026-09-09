"""
Test runner for NEXUS
"""
import sys
import os
import subprocess

def run_tests():
    """Run all tests"""
    print("[NEXUS] Running NEXUS Tests")
    print("=" * 40)

    # Change to the Jarvis directory
    jarvis_dir = os.path.dirname(os.path.abspath(__file__))
    os.chdir(jarvis_dir)

    # Run the basic import test
    test_path = os.path.join(jarvis_dir, "tests", "test_basic_import.py")
    print(f"Running: {test_path}")
    result = subprocess.run([sys.executable, test_path], capture_output=True, text=True, encoding='utf-8')

    print(result.stdout)
    if result.stderr:
        print("STDERR:", result.stderr)

    if result.returncode != 0:
        print("[FAIL] Tests failed!")
        return False
    else:
        print("[PASS] All tests passed!")
        return True

if __name__ == "__main__":
    success = run_tests()
    sys.exit(0 if success else 1)