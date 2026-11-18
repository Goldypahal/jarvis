"""
NEXUS: Neural Executive Understanding System
An advanced AI personal assistant designed to amplify human potential.
"""

__version__ = "0.1.0"
__author__ = "NEXUS Project"
__email__ = "nexus@example.com"

from .core.reasoning import ReasoningEngine
from .core.context import ContextManager
from .core.memory import MemorySystem

__all__ = [
    "ReasoningEngine",
    "ContextManager",
    "MemorySystem",
    "__version__",
]