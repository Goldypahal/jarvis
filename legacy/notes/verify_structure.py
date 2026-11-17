"""
Verify NEXUS project structure
"""
import os
import sys
import ast

def check_python_syntax(file_path):
    """Check if a Python file has valid syntax"""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        ast.parse(content)
        return True, None
    except SyntaxError as e:
        return False, str(e)
    except Exception as e:
        return False, str(e)

def main():
    """Verify the project structure"""
    print("[NEXUS] Verifying NEXUS Project Structure")
    print("=" * 50)

    jarvis_dir = os.path.dirname(os.path.abspath(__file__))

    # List of key files to check
    key_files = [
        "nexus/__init__.py",
        "nexus/__main__.py",
        "nexus/core/__init__.py",
        "nexus/core/reasoning.py",
        "nexus/core/context.py",
        "nexus/core/memory.py",
        "nexus/modules/__init__.py",
        "nexus/modules/task_orchestration.py",
        "nexus/modules/information_synthesis.py",
        "nexus/modules/creative_collaboration.py",
        "nexus/modules/decision_support.py",
        "nexus/modules/specialized_modes.py",
        "nexus/interfaces/__init__.py",
        "nexus/interfaces/console.py",
        "tests/test_basic_import.py",
        "examples/basic_usage.py",
        "run_tests.py"
    ]

    all_passed = True

    for file_path in key_files:
        full_path = os.path.join(jarvis_dir, file_path)
        if os.path.exists(full_path):
            passed, error = check_python_syntax(full_path)
            if passed:
                print(f"[OK] {file_path}")
            else:
                print(f"[FAIL] {file_path}: {error}")
                all_passed = False
        else:
            print(f"[MISSING] {file_path}: File not found")
            all_passed = False

    # Check directories
    key_dirs = [
        "nexus",
        "nexus/core",
        "nexus/modules",
        "nexus/interfaces",
        "nexus/config",
        "tests",
        "examples"
    ]

    print("\n[NEXUS] Checking Directories:")
    for dir_path in key_dirs:
        full_path = os.path.join(jarvis_dir, dir_path)
        if os.path.isdir(full_path):
            print(f"[OK] {dir_path}/")
        else:
            print(f"[FAIL] {dir_path}/: Directory not found")
            all_passed = False

    print("\n" + "=" * 50)
    if all_passed:
        print("[SUCCESS] All checks passed! Project structure is valid.")
        print("\nTo start NEXUS, run:")
        print("   python -m nexus")
        print("   or after installation: nexus")
    else:
        print("[FAIL] Some checks failed. Please review the errors above.")

    return all_passed

if __name__ == "__main__":
    success = main()
    sys.exit(0 if success else 1)