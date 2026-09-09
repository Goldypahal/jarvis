"""
Creative collaboration mode for NEXUS
Facilitates brainstorming, ideation, and creative problem-solving
"""

from typing import Any, Dict, List, Optional
from datetime import datetime
import asyncio
import logging
import random
from dataclasses import dataclass, field
from enum import Enum

from ..core.memory import MemoryEntry, MemoryType, MemorySystem

logger = logging.getLogger(__name__)


class CreativityTechnique(Enum):
    """Different creativity techniques"""
    BRAINSTORMING = "brainstorming"
    SCAMPER = "scamper"  # Substitute, Combine, Adapt, Modify, Put to another use, Eliminate, Reverse
    ROLE_STORMING = "role_storming"
    RANDOM_WORD = "random_word"
    MIND_MAP = "mind_map"
    SIX_THINKING_HATS = "six_thinking_hats"
    LATERAL_THINKING = "lateral_thinking"
    ANALOGICAL_REASONING = "analogical_reasoning"


class IdeaStatus(Enum):
    """Status of an idea in the creative process"""
    RAW = "raw"                    # Just generated
    EXPLORED = "explored"          # Some initial exploration
    REFINED = "refined"            # Has been developed further
    SELECTED = "selected"          # Chosen for pursuit
    IMPLEMENTED = "implemented"    # Has been implemented
    DISCARDED = "discarded"        # Rejected after evaluation


@dataclass
class CreativeIdea:
    """Represents a creative idea"""
    id: str
    title: str
    description: str
    origin: str  # How it was generated (technique, prompt, etc.)
    status: IdeaStatus = IdeaStatus.RAW
    tags: List[str] = field(default_factory=list)
    related_ideas: List[str] = field(default_factory=list)  # IDs of related ideas
    source_concepts: List[str] = field(default_factory=list)  # What inspired it
    potential_impact: float = 0.5  # 0.0 to 1.0
    feasibility: float = 0.5  # 0.0 to 1.0
    novelty: float = 0.5  # 0.0 to 1.0
    created_at: datetime = field(default_factory=datetime.now)
    updated_at: datetime = field(default_factory=datetime.now)
    creator_notes: str = ""
    evaluation_scores: Dict[str, float] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for serialization"""
        return {
            "id": self.id,
            "title": self.title,
            "description": self.description,
            "origin": self.origin,
            "status": self.status.value,
            "tags": self.tags,
            "related_ideas": self.related_ideas,
            "source_concepts": self.source_concepts,
            "potential_impact": self.potential_impact,
            "feasibility": self.feasibility,
            "novelty": self.novelty,
            "created_at": self.created_at.isoformat(),
            "updated_at": self.updated_at.isoformat(),
            "creator_notes": self.creator_notes,
            "evaluation_scores": self.evaluation_scores
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> 'CreativeIdea':
        """Create from dictionary"""
        data = data.copy()
        data['status'] = IdeaStatus(data['status'])
        if data.get('created_at'):
            data['created_at'] = datetime.fromisoformat(data['created_at'])
        if data.get('updated_at'):
            data['updated_at'] = datetime.fromisoformat(data['updated_at'])
        return cls(**data)


@dataclass
class BrainstormSession:
    """Represents a brainstorming session"""
    id: str
    topic: str
    goal: str
    techniques_used: List[CreativityTechnique] = field(default_factory=list)
    ideas: List[str] = field(default_factory=list)  # IDs of CreativeIdea objects
    participant_count: int = 1  # For solo sessions with NEXUS
    started_at: datetime = field(default_factory=datetime.now)
    ended_at: Optional[datetime] = None
    summary: str = ""
    outcomes: List[str] = field(default_factory=list)


class CreativeCollaborator:
    """
    Facilitates creative collaboration through brainstorming, ideation,
    and creative problem-solving techniques.
    """

    def __init__(self, memory_system: Optional['MemorySystem'] = None):
        self.memory_system = memory_system
        self.ideas: Dict[str, CreativeIdea] = {}
        self.sessions: Dict[str, BrainstormSession] = {}
        self.idea_counter = 0
        self.session_counter = 0

        # Word bank for random word technique
        self.random_words = [
            "apple", "ocean", "mountain", "music", "dance", "robot", "garden",
            "kitchen", "space", "time", "light", "sound", "movement", "balance",
            "growth", "network", "foundation", "transformation", "connection",
            "flow", "energy", "pattern", "structure", "system", "cycle"
        ]

    def _generate_idea_id(self) -> str:
        """Generate a unique idea ID"""
        self.idea_counter += 1
        return f"idea_{self.idea_counter:04d}"

    def _generate_session_id(self) -> str:
        """Generate a unique session ID"""
        self.session_counter += 1
        return f"session_{self.session_counter:04d}"

    async def brainstorm(
        self,
        topic: str,
        goal: Optional[str] = None,
        technique: CreativityTechnique = CreativityTechnique.BRAINSTORMING,
        session_id: Optional[str] = None,
        num_ideas: int = 10,
        context: Optional[Dict[str, Any]] = None
    ) -> Dict[str, Any]:
        """
        Conduct a brainstorming session to generate ideas.

        Args:
            topic: The topic to brainstorm about
            goal: Optional specific goal for the brainstorming
            technique: Creativity technique to use
            session_id: Optional session ID to continue an existing session
            num_ideas: Number of ideas to generate
            context: Optional contextual information

        Returns:
            Dictionary containing brainstorming results
        """
        # Get or create session
        if session_id and session_id in self.sessions:
            session = self.sessions[session_id]
            logger.info(f"Continuing brainstorming session {session_id}")
        else:
            session_id = self._generate_session_id()
            session = BrainstormSession(
                id=session_id,
                topic=topic,
                goal=goal or f"Generate ideas for {topic}",
                participant_count=1  # Solo session with NEXUS
            )
            self.sessions[session_id] = session
            logger.info(f"Started new brainstorming session {session_id}")

        # Ensure valid technique
        if technique is None:
            technique = CreativityTechnique.BRAINSTORMING

        # Record technique used
        if technique not in session.techniques_used:
            session.techniques_used.append(technique)

        # Generate ideas using the selected technique
        ideas = await self._generate_ideas_with_technique(
            topic, goal, technique, num_ideas, context
        )

        # Add ideas to session
        idea_ids = [idea.id for idea in ideas]
        session.ideas.extend(idea_ids)

        # Store ideas in memory
        for idea in ideas:
            self.ideas[idea.id] = idea
            if self.memory_system:
                memory_entry = MemoryEntry(
                    id=f"idea_{idea.id}",
                    memory_type=MemoryType.PROCEDURAL,
                    content=idea.to_dict(),
                    importance_score=0.5 + idea.novelty * 0.3,  # Novel ideas more important
                    tags=["idea", topic.replace(" ", "_")],
                    expires_at=None
                )
                await self.memory_system.store_memory(memory_entry)

        # Update session
        session.updated_at = datetime.now()

        # Generate session summary
        session_summary = await self._generate_session_summary(session)
        session.summary = session_summary

        result = {
            "session_id": session_id,
            "topic": topic,
            "technique_used": technique.value,
            "ideas_generated": len(ideas),
            "ideas": [idea.to_dict() for idea in ideas],
            "session_summary": session_summary,
            "next_steps": await self._suggest_next_creative_steps(session, ideas)
        }

        logger.info(f"Brainstorming session {session_id} generated {len(ideas)} ideas")
        return result

    async def _generate_ideas_with_technique(
        self,
        topic: str,
        goal: Optional[str],
        technique: CreativityTechnique,
        num_ideas: int,
        context: Optional[Dict[str, Any]]
    ) -> List[CreativeIdea]:
        """
        Generate ideas using a specific creativity technique.

        Args:
            topic: The topic to brainstorm about
            goal: Optional specific goal
            technique: Creativity technique to use
            num_ideas: Number of ideas to generate
            context: Optional contextual information

        Returns:
            List of CreativeIdea objects
        """
        ideas = []

        if technique == CreativityTechnique.BRAINSTORMING:
            ideas = await self._brainstorming_technique(topic, goal, num_ideas, context)
        elif technique == CreativityTechnique.SCAMPER:
            ideas = await self._scamper_technique(topic, goal, num_ideas, context)
        elif technique == CreativityTechnique.RANDOM_WORD:
            ideas = await self._random_word_technique(topic, goal, num_ideas, context)
        elif technique == CreativityTechnique.ANALOGICAL_REASONING:
            ideas = await self._analogical_reasoning_technique(topic, goal, num_ideas, context)
        elif technique == CreativityTechnique.SIX_THINKING_HATS:
            ideas = await self._six_thinking_hats_technique(topic, goal, num_ideas, context)
        else:
            # Default to brainstorming for unimplemented techniques
            ideas = await self._brainstorming_technique(topic, goal, num_ideas, context)

        return ideas

    async def _brainstorming_technique(
        self,
        topic: str,
        goal: Optional[str],
        num_ideas: int,
        context: Optional[Dict[str, Any]]
    ) -> List[CreativeIdea]:
        """Standard brainstorming technique"""
        ideas = []
        prompts = [
            f"How might we improve {topic}?",
            f"What if we could change one thing about {topic}?",
            f"What are the biggest challenges with {topic} and how to overcome them?",
            f"How could {topic} be made more accessible/efficient/enjoyable?",
            f"What would {topic} look like in 10 years?",
            f"What if we had unlimited resources for {topic}?",
            f"How do other industries approach similar problems to {topic}?",
            f"What assumptions about {topic} might we be wrong about?",
            f"How could we make {topic} more sustainable?",
            f"What would a beginner/expert suggest for {topic}?"
        ]

        for i in range(min(num_ideas, len(prompts))):
            idea = CreativeIdea(
                id=self._generate_idea_id(),
                title=f"Idea {i+1}: {topic}",
                description=f"Generated via brainstorming: {prompts[i]}",
                origin="brainstorming",
                tags=self._extract_tags_from_topic(topic),
                potential_impact=random.uniform(0.4, 0.9),
                feasibility=random.uniform(0.3, 0.8),
                novelty=random.uniform(0.3, 0.8)
            )
            ideas.append(idea)

        return ideas

    async def _scamper_technique(
        self,
        topic: str,
        goal: Optional[str],
        num_ideas: int,
        context: Optional[Dict[str, Any]]
    ) -> List[CreativeIdea]:
        """SCAMPER technique: Substitute, Combine, Adapt, Modify, Put to another use, Eliminate, Reverse"""
        ideas = []
        scamper_prompts = [
            f"What could we substitute in {topic} to make it better?",
            f"What could we combine {topic} with to create something new?",
            f"How could we adapt {topic} for a different context or use?",
            f"What could we modify or change about {topic}?",
            f"How could we put {topic} to a different use?",
            f"What could we eliminate from {topic} to simplify or improve it?",
            f"What would happen if we reversed the process or direction of {topic}?"
        ]

        for i in range(min(num_ideas, len(scamper_prompts))):
            idea = CreativeIdea(
                id=self._generate_idea_id(),
                title=f"SCAMPER Idea {i+1}: {topic}",
                description=f"Generated via SCAMPER: {scamper_prompts[i]}",
                origin="scamper",
                tags=self._extract_tags_from_topic(topic) + ["scamper"],
                potential_impact=random.uniform(0.5, 0.9),
                feasibility=random.uniform(0.4, 0.8),
                novelty=random.uniform(0.4, 0.9)
            )
            ideas.append(idea)

        return ideas

    async def _random_word_technique(
        self,
        topic: str,
        goal: Optional[str],
        num_ideas: int,
        context: Optional[Dict[str, Any]]
    ) -> List[CreativeIdea]:
        """Random word technique to trigger novel associations"""
        ideas = []

        for i in range(num_ideas):
            random_word = random.choice(self.random_words)
            idea = CreativeIdea(
                id=self._generate_idea_id(),
                title=f"Random Word Idea {i+1}: {topic} + {random_word}",
                description=f"Connecting {topic} with random word '{random_word}' to generate novel associations",
                origin="random_word",
                tags=self._extract_tags_from_topic(topic) + ["random_word", random_word.lower()],
                source_concepts=[random_word],
                potential_impact=random.uniform(0.3, 0.8),
                feasibility=random.uniform(0.2, 0.7),
                novelty=random.uniform(0.5, 0.95)  # High novelty potential
            )
            ideas.append(idea)

        return ideas

    async def _analogical_reasoning_technique(
        self,
        topic: str,
        goal: Optional[str],
        num_ideas: int,
        context: Optional[Dict[str, Any]]
    ) -> List[CreativeIdea]:
        """Analogical reasoning: drawing parallels from different domains"""
        ideas = []
        domains = [
            "nature/ecology", "music/art", "cooking/food", "sports/games",
            "architecture/design", "transportation/logistics", "medicine/healthcare",
            "education/teaching", "social_systems", "computer_science"
        ]

        for i in range(min(num_ideas, len(domains))):
            domain = domains[i]
            idea = CreativeIdea(
                id=self._generate_idea_id(),
                title=f"Analogical Idea {i+1}: {topic} inspired by {domain}",
                description=f"How principles from {domain} could be applied to {topic}",
                origin="analogical_reasoning",
                tags=self._extract_tags_from_topic(topic) + ["analogical", domain.replace("/", "_")],
                source_concepts=[domain],
                potential_impact=random.uniform(0.4, 0.8),
                feasibility=random.uniform(0.3, 0.7),
                novelty=random.uniform(0.5, 0.9)
            )
            ideas.append(idea)

        return ideas

    async def _six_thinking_hats_technique(
        self,
        topic: str,
        goal: Optional[str],
        num_ideas: int,
        context: Optional[Dict[str, Any]]
    ) -> List[CreativeIdea]:
        """Six Thinking Hat technique: different perspectives"""
        ideas = []
        hats = [
            ("White Hat", "Facts and information: What do we know about {topic}?"),
            ("Red Hat", "Emotions and feelings: How do people feel about {topic}?"),
            ("Black Hat", "Critical judgment: What are the risks and downsides of {topic}?"),
            ("Yellow Hat", "Positive benefits: What are the advantages and value of {topic}?"),
            ("Green Hat", "Creative thinking: What new ideas could we have about {topic}?"),
            ("Blue Hat", "Process control: How should we approach thinking about {topic}?")
        ]

        for i in range(min(num_ideas, len(hats))):
            hat_name, prompt_template = hats[i]
            prompt = prompt_template.format(topic=topic)
            idea = CreativeIdea(
                id=self._generate_idea_id(),
                title=f"{hat_name} Idea {i+1}: {topic}",
                description=f"Generated via Six Thinking Hats ({hat_name}): {prompt}",
                origin="six_thinking_hats",
                tags=self._extract_tags_from_topic(topic) + ["six_hats", hat_name.lower().replace(" ", "_")],
                potential_impact=random.uniform(0.4, 0.8),
                feasibility=random.uniform(0.3, 0.8),
                novelty=random.uniform(0.3, 0.7)
            )
            ideas.append(idea)

        return ideas

    def _extract_tags_from_topic(self, topic: str) -> List[str]:
        """Extract relevant tags from a topic string"""
        # Simple implementation - in reality would use NLP
        words = topic.lower().split()
        # Filter out common words
        stop_words = {"the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by"}
        tags = [word.strip(".,!?;:") for word in words if word not in stop_words and len(word) > 2]
        return tags[:5]  # Limit tags

    async def _generate_session_summary(
        self,
        session: BrainstormSession
    ) -> str:
        """Generate a summary of the brainstorming session"""
        idea_count = len(session.ideas)
        techniques_list = [t.value if hasattr(t, 'value') else str(t) for t in session.techniques_used if t is not None]
        techniques = ", ".join(techniques_list) if techniques_list else "None"

        summary = f"""
Brainstorming Session Summary:
- Topic: {session.topic}
- Goal: {session.goal}
- Ideas Generated: {idea_count}
- Techniques Used: {techniques}
- Session Duration: {(datetime.now() - session.started_at).total_seconds() / 60:.1f} minutes

Key Themes Observed:
- Focus on practical applicability
- Exploration of novel combinations
- Consideration of user experience and impact
"""

        return summary.strip()

    async def _suggest_next_creative_steps(
        self,
        session: BrainstormSession,
        ideas: List[CreativeIdea]
    ) -> List[str]:
        """Suggest next steps after brainstorming"""
        suggestions = []

        if ideas:
            # Sort ideas by potential
            sorted_ideas = sorted(ideas,
                                key=lambda i: (i.potential_impact * 0.4 +
                                             i.novelty * 0.3 +
                                             i.feasibility * 0.3),
                                reverse=True)

            top_idea = sorted_ideas[0]
            suggestions.append(f"Explore and refine the top idea: '{top_idea.title}'")
            suggestions.append(f"Combine elements from the top 3 ideas to create hybrid solutions")
            suggestions.append(f"Apply the SCAMPER technique to the most promising ideas")
            suggestions.append(f"Create quick prototypes or sketches for the top 2 ideas")
            suggestions.append(f"Seek feedback from stakeholders on the most innovative concepts")

        else:
            suggestions.append("Generate more ideas using different creativity techniques")
            suggestions.append("Take a break and return with fresh perspective")
            suggestions.append("Research the topic more deeply to stimulate new associations")

        return suggestions

    async def refine_idea(
        self,
        idea_id: str,
        refinement_notes: str,
        impact_estimate: Optional[float] = None,
        feasibility_estimate: Optional[float] = None
    ) -> bool:
        """
        Refine an existing idea.

        Args:
            idea_id: ID of idea to refine
            refinement_notes: Notes about the refinement
            impact_estimate: Updated impact estimate (0.0 to 1.0)
            feasibility_estimate: Updated feasibility estimate (0.0 to 1.0)

        Returns:
            True if refined, False if idea not found
        """
        if idea_id not in self.ideas:
            return False

        idea = self.ideas[idea_id]
        idea.creator_notes = refinement_notes
        idea.updated_at = datetime.now()

        if impact_estimate is not None:
            idea.potential_impact = max(0.0, min(1.0, impact_estimate))
        if feasibility_estimate is not None:
            idea.feasibility = max(0.0, min(1.0, feasibility_estimate))

        # Move to refined status if was raw or explored
        if idea.status in [IdeaStatus.RAW, IdeaStatus.EXPLORED]:
            idea.status = IdeaStatus.REFINED

        # Update in memory
        if self.memory_system:
            memory_entry = MemoryEntry(
                id=f"idea_{idea.id}",
                memory_type=MemoryType.PROCEDURAL,
                content=idea.to_dict(),
                importance_score=0.6 + idea.novelty * 0.3,
                tags=["idea", "refined"],
                expires_at=None
            )
            await self.memory_system.store_memory(memory_entry)

        logger.info(f"Refined idea {idea_id}")
        return True

    async def evaluate_idea(
        self,
        idea_id: str,
        criteria_scores: Dict[str, float]
    ) -> bool:
        """
        Evaluate an idea against specific criteria.

        Args:
            idea_id: ID of idea to evaluate
            criteria_scores: Dictionary of criterion names to scores (0.0 to 1.0)

        Returns:
            True if evaluated, False if idea not found
        """
        if idea_id not in self.ideas:
            return False

        idea = self.ideas[idea_id]
        idea.evaluation_scores.update(criteria_scores)
        idea.updated_at = datetime.now()

        # Calculate overall score if we have criteria
        if criteria_scores:
            idea.novelty = criteria_scores.get("novelty", idea.novelty)
            idea.feasibility = criteria_scores.get("feasibility", idea.feasibility)
            idea.potential_impact = criteria_scores.get("impact", idea.potential_impact)

        # Move to selected status if scores are high enough
        avg_score = sum(criteria_scores.values()) / len(criteria_scores) if criteria_scores else 0.5
        if avg_score >= 0.7 and idea.status == IdeaStatus.REFINED:
            idea.status = IdeaStatus.SELECTED

        # Update in memory
        if self.memory_system:
            memory_entry = MemoryEntry(
                id=f"idea_{idea.id}",
                memory_type=MemoryType.PROCEDURAL,
                content=idea.to_dict(),
                importance_score=0.7 + idea.novelty * 0.2,
                tags=["idea", "evaluated"],
                expires_at=None
            )
            await self.memory_system.store_memory(memory_entry)

        logger.info(f"Evaluated idea {idea_id}")
        return True

    async def get_idea(self, idea_id: str) -> Optional[CreativeIdea]:
        """Get an idea by ID"""
        return self.ideas.get(idea_id)

    async def get_session_ideas(self, session_id: str) -> List[CreativeIdea]:
        """Get all ideas from a brainstorming session"""
        if session_id not in self.sessions:
            return []

        session = self.sessions[session_id]
        ideas = []
        for idea_id in session.ideas:
            if idea_id in self.ideas:
                ideas.append(self.ideas[idea_id])
        return ideas

    async def get_top_ideas(
        self,
        limit: int = 10,
        sort_by: str = "potential_impact"  # potential_impact, feasibility, novelty, combined
    ) -> List[CreativeIdea]:
        """
        Get top ideas sorted by specified criteria.

        Args:
            limit: Maximum number of ideas to return
            sort_by: Criteria to sort by

        Returns:
            List of top CreativeIdea objects
        """
        ideas = list(self.ideas.values())

        if sort_by == "potential_impact":
            ideas.sort(key=lambda i: i.potential_impact, reverse=True)
        elif sort_by == "feasibility":
            ideas.sort(key=lambda i: i.feasibility, reverse=True)
        elif sort_by == "novelty":
            ideas.sort(key=lambda i: i.novelty, reverse=True)
        elif sort_by == "combined":
            ideas.sort(key=lambda i: (i.potential_impact * 0.4 +
                                    i.novelty * 0.3 +
                                    i.feasibility * 0.3), reverse=True)
        else:
            # Default to combined score
            ideas.sort(key=lambda i: (i.potential_impact * 0.4 +
                                    i.novelty * 0.3 +
                                    i.feasibility * 0.3), reverse=True)

        return ideas[:limit]


# Example usage
if __name__ == "__main__":
    import asyncio

    async def main():
        # Initialize creative collaborator
        collaborator = CreativeCollaborator()

        # Conduct a brainstorming session
        print("=== Brainstorming Session ===")
        brainstorm_result = await collaborator.brainstorm(
            topic="Improving remote team collaboration",
            goal="Generate ideas to make remote work more effective and enjoyable",
            technique=CreativityTechnique.BRAINSTORMING,
            num_ideas=8
        )

        print(f"Session ID: {brainstorm_result['session_id']}")
        print(f"Topic: {brainstorm_result['topic']}")
        print(f"Ideas Generated: {brainstorm_result['ideas_generated']}")
        print(f"\nSession Summary:\n{brainstorm_result['session_summary']}")

        print("\nTop 3 Ideas:")
        for i, idea_dict in enumerate(brainstorm_result['ideas'][:3], 1):
            print(f"{i}. {idea_dict['title']}")
            print(f"   Description: {idea_dict['description']}")
            print(f"   Impact: {idea_dict['potential_impact']:.2f}, "
                  f"Feasibility: {idea_dict['feasibility']:.2f}, "
                  f"Novelty: {idea_dict['novelty']:.2f}")

        # Try a different technique
        print("\n\n=== SCAMPER Technique ===")
        scamper_result = await collaborator.brainstorm(
            topic="Mobile app for learning languages",
            goal="Innovate language learning app features",
            technique=CreativityTechnique.SCAMPER,
            num_ideas=5,
            session_id=brainstorm_result['session_id']  # Continue same session
        )

        print(f"SCAMPER Ideas Generated: {scamper_result['ideas_generated']}")

        # Refine an idea
        if brainstorm_result['ideas']:
            first_idea = brainstorm_result['ideas'][0]
            idea_id = first_idea['id']
            print(f"\n\n=== Refining Idea {idea_id} ===")
            refined = await collaborator.refine_idea(
                idea_id=idea_id,
                refinement_notes="Added gamification elements and AI-powered feedback",
                impact_estimate=0.85,
                feasibility_estimate=0.7
            )
            print(f"Idea refined: {refined}")

            # Evaluate the idea
            print(f"\n=== Evaluating Idea {idea_id} ===")
            evaluated = await collaborator.evaluate_idea(
                idea_id=idea_id,
                criteria_scores={
                    "novelty": 0.8,
                    "feasibility": 0.7,
                    "impact": 0.85,
                    "user_value": 0.9,
                    "technical_difficulty": 0.3  # Lower is better for this criterion
                }
            )
            print(f"Idea evaluated: {evaluated}")

            # Get the updated idea
            idea = await collaborator.get_idea(idea_id)
            if idea:
                print(f"Updated Idea Status: {idea.status.value}")
                print(f"Evaluation Scores: {idea.evaluation_scores}")

        # Show top ideas
        print("\n\n=== Top Ideas Across All Sessions ===")
        top_ideas = await collaborator.get_top_ideas(limit=5, sort_by="combined")
        for i, idea in enumerate(top_ideas, 1):
            print(f"{i}. {idea.title}")
            print(f"   Status: {idea.status.value}")
            print(f"   Combined Score: {(idea.potential_impact * 0.4 + idea.novelty * 0.3 + idea.feasibility * 0.3):.2f}")

    asyncio.run(main())