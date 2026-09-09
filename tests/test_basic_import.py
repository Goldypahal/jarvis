"""
Basic import test for NEXUS modules
"""
import sys
import os

# Add the nexus package to the path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

def test_imports():
    """Test that we can import the main modules"""
    try:
        # Test core imports
        from nexus.core.reasoning import ReasoningEngine
        from nexus.core.context import ContextManager
        from nexus.core.memory import MemorySystem
        print("[OK] Core modules imported successfully")

        # Test module imports
        from nexus.modules.task_orchestration import TaskOrchestrator
        from nexus.modules.information_synthesis import InformationSynthesizer
        from nexus.modules.creative_collaboration import CreativeCollaborator
        from nexus.modules.decision_support import DecisionAnalyzer
        from nexus.modules.specialized_modes import SpecializedModes, ModeType
        print("[OK] Module imports successful")

        # Test interface imports
        from nexus.interfaces.console import NexusConsole
        print("[OK] Interface imports successful")

        # Test that we can create instances
        memory = MemorySystem("test_memory.db")
        context = ContextManager()
        reasoning = ReasoningEngine(context)
        print("[OK] Basic instantiation successful")

        # Clean up
        memory.close()
        if os.path.exists("test_memory.db"):
            os.remove("test_memory.db")

        print("\n[SUCCESS] All basic tests passed!")
        return True

    except Exception as e:
        print(f"[FAIL] Test failed: {e}")
        import traceback
        traceback.print_exc()
        return False

if __name__ == "__main__":
    success = test_imports()
    sys.exit(0 if success else 1)