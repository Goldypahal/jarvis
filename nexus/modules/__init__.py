"""
NEXUS functional modules
"""
from .skill_learning import SkillLearner, SkillType, SkillLevel

__all__ = [
    "SkillLearner",
    "SkillType",
    "SkillLevel",
    # Existing exports will be added back when we update this properly
]

# Import existing modules to maintain compatibility
try:
    from .task_orchestration import TaskOrchestrator
    from .information_synthesis import InformationSynthesizer
    from .creative_collaboration import CreativeCollaborator
    from .decision_support import DecisionAnalyzer
    from .specialized_modes import SpecializedModes, ModeType

    __all__.extend([
        "TaskOrchestrator",
        "InformationSynthesizer",
        "CreativeCollaborator",
        "DecisionAnalyzer",
        "SpecializedModes",
        "ModeType"
    ])
except ImportError as e:
    # In case some modules aren't available yet during initial setup
    pass