"""
Decision support architecture for NEXUS
Provides structured decision analysis, trade-off evaluation, and recommendation generation
"""

from typing import Any, Dict, List, Optional
from datetime import datetime
import asyncio
import logging
import math
from dataclasses import dataclass, field
from enum import Enum

from ..core.memory import MemoryEntry, MemoryType, MemorySystem

logger = logging.getLogger(__name__)


class DecisionType(Enum):
    """Types of decisions"""
    STRATEGIC = "strategic"      # Long-term, high-impact decisions
    TACTICAL = "tactical"        # Medium-term, operational decisions
    OPERATIONAL = "operational"  # Short-term, routine decisions
    PERSONAL = "personal"        # Personal life decisions
    FINANCIAL = "financial"      # Money-related decisions
    TECHNICAL = "technical"      # Technology-related decisions


class RecommendationStrength(Enum):
    """Strength of a recommendation"""
    WEAK = "weak"          # Slight preference, easily overridden
    MODERATE = "moderate"  # Clear preference but with notable trade-offs
    STRONG = "strong"      # Clear preference with minimal downsides
    VERY_STRONG = "very_strong"  # Overwhelmingly clear choice


@dataclass
class DecisionCriterion:
    """Represents a criterion for evaluating decision options"""
    id: str
    name: str
    description: str
    weight: float  # 0.0 to 1.0, relative importance
    is_benefit: bool = True  # True if higher is better, False if lower is better
    weight_normalized: float = field(init=False)

    def __post_init__(self):
        # Weight normalization will be done by DecisionAnalyzer
        self.weight_normalized = self.weight


@dataclass
class DecisionOption:
    """Represents an option in a decision"""
    id: str
    title: str
    description: str
    scores: Dict[str, float] = field(default_factory=dict)  # criterion_id -> score (0.0 to 1.0)
    pros: List[str] = field(default_factory=list)
    cons: List[str] = field(default_factory=list)
    risks: List[str] = field(default_factory=list)
    assumptions: List[str] = field(default_factory=list)
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for serialization"""
        return {
            "id": self.id,
            "title": self.title,
            "description": self.description,
            "scores": self.scores,
            "pros": self.pros,
            "cons": self.cons,
            "risks": self.risks,
            "assumptions": self.assumptions,
            "metadata": self.metadata
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> 'DecisionOption':
        """Create from dictionary"""
        return cls(**data)


@dataclass
class DecisionAnalysis:
    """Result of a decision analysis"""
    id: str
    decision_title: str
    decision_type: DecisionType
    options: List[DecisionOption] = field(default_factory=list)
    criteria: List[DecisionCriterion] = field(default_factory=list)
    recommended_option_id: Optional[str] = None
    recommendation_strength: Optional[RecommendationStrength] = None
    detailed_scores: Dict[str, Dict[str, float]] = field(default_factory=dict)  # option_id -> criterion_id -> weighted_score
    weighted_totals: Dict[str, float] = field(default_factory=dict)  # option_id -> total weighted score
    trade_offs: List[Dict[str, Any]] = field(default_factory=list)
    sensitivity_analysis: Dict[str, Any] = field(default_factory=dict)
    assumptions: List[str] = field(default_factory=list)
    created_at: datetime = field(default_factory=datetime.now)
    confidence: float = 0.5  # 0.0 to 1.0

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for serialization"""
        return {
            "id": self.id,
            "decision_title": self.decision_title,
            "decision_type": self.decision_type.value,
            "options": [option.to_dict() for option in self.options],
            "criteria": [
                {
                    "id": c.id,
                    "name": c.name,
                    "description": c.description,
                    "weight": c.weight,
                    "is_benefit": c.is_benefit,
                    "weight_normalized": c.weight_normalized
                }
                for c in self.criteria
            ],
            "recommended_option_id": self.recommended_option_id,
            "recommendation_strength": self.recommendation_strength.value if self.recommendation_strength else None,
            "detailed_scores": self.detailed_scores,
            "weighted_totals": self.weighted_totals,
            "trade_offs": self.trade_offs,
            "sensitivity_analysis": self.sensitivity_analysis,
            "assumptions": self.assumptions,
            "created_at": self.created_at.isoformat(),
            "confidence": self.confidence
        }


class DecisionAnalyzer:
    """
    Provides structured decision analysis capabilities including:
    - Multi-criteria decision analysis (MCDA)
    - Trade-off identification
    - Sensitivity analysis
    - Recommendation generation
    """

    def __init__(self, memory_system: Optional['MemorySystem'] = None):
        self.memory_system = memory_system
        self.analysis_counter = 0

    def _generate_analysis_id(self) -> str:
        """Generate a unique analysis ID"""
        self.analysis_counter += 1
        return f"decision_{self.analysis_counter:04d}"

    async def analyze_decision(
        self,
        decision_title: str,
        decision_type: DecisionType,
        options: List[DecisionOption],
        criteria: List[DecisionCriterion],
        context: Optional[Dict[str, Any]] = None
    ) -> DecisionAnalysis:
        """
        Perform a multi-criteria decision analysis.

        Args:
            decision_title: Title of the decision
            decision_type: Type of decision
            options: List of options to evaluate
            criteria: List of criteria for evaluation
            context: Optional contextual information

        Returns:
            DecisionAnalysis object with results
        """
        logger.info(f"Analyzing decision: {decision_title} with {len(options)} options and {len(criteria)} criteria")

        # Normalize criterion weights
        self._normalize_weights(criteria)

        # Calculate scores for each option
        detailed_scores, weighted_totals = await self._calculate_scores(options, criteria)

        # Generate trade-off analysis
        trade_offs = await self._analyze_trade_offs(options, criteria, detailed_scores)

        # Perform sensitivity analysis
        sensitivity_analysis = await self._perform_sensitivity_analysis(options, criteria, weighted_totals)

        # Determine recommendation
        recommended_option_id, recommendation_strength, confidence = await self._generate_recommendation(
            options, criteria, weighted_totals, trade_offs
        )

        # Create analysis result
        analysis = DecisionAnalysis(
            id=self._generate_analysis_id(),
            decision_title=decision_title,
            decision_type=decision_type,
            options=options,
            criteria=criteria,
            recommended_option_id=recommended_option_id,
            recommendation_strength=recommendation_strength,
            detailed_scores=detailed_scores,
            weighted_totals=weighted_totals,
            trade_offs=trade_offs,
            sensitivity_analysis=sensitivity_analysis,
            assumptions=[
                "Criteria weights accurately reflect decision maker's preferences",
                "Option scores are accurate and comparable",
                "No significant missing criteria or options",
                "Decision context is stable during analysis"
            ],
            confidence=confidence
        )

        # Store in memory if available
        if self.memory_system:
            memory_entry = MemoryEntry(
                id=f"decision_analysis_{analysis.id}",
                memory_type=MemoryType.SEMANTIC,
                content=analysis.to_dict(),
                importance_score=0.7,
                tags=["decision", decision_type.value],
                expires_at=None
            )
            await self.memory_system.store_memory(memory_entry)

        logger.info(f"Decision analysis completed. Recommended option: {recommended_option_id}")
        return analysis

    def _normalize_weights(self, criteria: List[DecisionCriterion]) -> None:
        """Normalize criterion weights to sum to 1.0"""
        total_weight = sum(criterion.weight for criterion in criteria)
        if total_weight > 0:
            for criterion in criteria:
                criterion.weight_normalized = criterion.weight / total_weight
        else:
            # Equal weights if all zero or no criteria
            equal_weight = 1.0 / len(criteria) if criteria else 0.0
            for criterion in criteria:
                criterion.weight_normalized = equal_weight

    async def _calculate_scores(
        self,
        options: List[DecisionOption],
        criteria: List[DecisionCriterion]
    ) -> tuple[Dict[str, Dict[str, float]], Dict[str, float]]:
        """
        Calculate detailed and weighted scores for each option.

        Returns:
            Tuple of (detailed_scores, weighted_totals)
            detailed_scores: option_id -> criterion_id -> normalized_score (0-1)
            weighted_totals: option_id -> total weighted score
        """
        detailed_scores = {}
        weighted_totals = {}

        for option in options:
            detailed_scores[option.id] = {}
            weighted_total = 0.0

            for criterion in criteria:
                # Get the option's score for this criterion (default to 0.5 if not provided)
                raw_score = option.scores.get(criterion.id, 0.5)
                # Ensure score is in [0, 1]
                normalized_score = max(0.0, min(1.0, raw_score))

                # For cost criteria (is_benefit=False), we invert the score
                # So that higher normalized_score always means better
                if not criterion.is_benefit:
                    normalized_score = 1.0 - normalized_score

                # Apply criterion weight
                weighted_score = normalized_score * criterion.weight_normalized

                detailed_scores[option.id][criterion.id] = weighted_score
                weighted_total += weighted_score

            weighted_totals[option.id] = weighted_total

        return detailed_scores, weighted_totals

    async def _analyze_trade_offs(
        self,
        options: List[DecisionOption],
        criteria: List[DecisionCriterion],
        detailed_scores: Dict[str, Dict[str, float]]
    ) -> List[Dict[str, Any]]:
        """
        Analyze trade-offs between options.

        Returns:
            List of trade-off dictionaries
        """
        trade_offs = []

        # Compare each pair of options
        for i in range(len(options)):
            for j in range(i + 1, len(options)):
                opt_a = options[i]
                opt_b = options[j]
                id_a = opt_a.id
                id_b = opt_b.id

                # Calculate overall performance difference
                total_a = detailed_scores[id_a]
                total_b = detailed_scores[id_b]

                # Find criteria where one option is significantly better than the other
                significant_differences = []
                for criterion in criteria:
                    crit_id = criterion.id
                    score_a = detailed_scores[id_a].get(crit_id, 0.0)
                    score_b = detailed_scores[id_b].get(crit_id, 0.0)
                    diff = score_a - score_b

                    # Consider a difference significant if > 0.1 (10% of weighted score)
                    if abs(diff) > 0.1:
                        better_option = id_a if diff > 0 else id_b
                        worse_option = id_b if diff > 0 else id_a
                        significant_differences.append({
                            "criterion_id": crit_id,
                            "criterion_name": criterion.name,
                            "difference": abs(diff),
                            "better_option": better_option,
                            "worse_option": worse_option,
                            "better_score": max(score_a, score_b),
                            "worse_score": min(score_a, score_b)
                        })

                if significant_differences:
                    trade_offs.append({
                        "option_a_id": id_a,
                        "option_a_title": opt_a.title,
                        "option_b_id": id_b,
                        "option_b_title": opt_b.title,
                        "significant_differences": significant_differences,
                        "summary": f"{opt_a.title} vs {opt_b.title}: {len(significant_differences)} criteria show significant differences"
                    })

        return trade_offs

    async def _perform_sensitivity_analysis(
        self,
        options: List[DecisionOption],
        criteria: List[DecisionCriterion],
        weighted_totals: Dict[str, float]
    ) -> Dict[str, Any]:
        """
        Perform sensitivity analysis on criterion weights.

        Returns:
            Dictionary with sensitivity analysis results
        """
        # Base case: current weighted totals
        base_totals = weighted_totals.copy()
        base_recommendation = max(base_totals, key=base_totals.get) if base_totals else None

        # Test sensitivity by varying each criterion's weight
        sensitivity_results = {}

        for criterion in criteria:
            crit_id = criterion.id
            original_weight = criterion.weight_normalized

            # Test extreme cases: set this criterion's weight to 0 and redistribute
            # Actually, we'll test by doubling and halving the weight (then renormalize)
            test_weights = [0.5 * original_weight, 2.0 * original_weight]
            test_labels = ["weight_halved", "weight_doubled"]

            for i, test_weight in enumerate(test_weights):
                # Create temporary criteria with modified weight
                temp_criteria = []
                for c in criteria:
                    temp_c = DecisionCriterion(
                        id=c.id,
                        name=c.name,
                        description=c.description,
                        weight=c.weight,  # Keep original weight for renormalization
                        is_benefit=c.is_benefit
                    )
                    temp_criteria.append(temp_c)

                # Modify the specific criterion's weight
                for temp_c in temp_criteria:
                    if temp_c.id == crit_id:
                        temp_c.weight = test_weight
                        break

                # Renormalize weights
                self._normalize_weights(temp_criteria)

                # Recalculate weighted totals
                _, test_totals = await self._calculate_scores(options, temp_criteria)
                test_recommendation = max(test_totals, key=test_totals.get) if test_totals else None

                # Check if recommendation changed
                recommendation_changed = (test_recommendation != base_recommendation)

                sensitivity_results[f"{crit_id}_{test_labels[i]}"] = {
                    "criterion_id": crit_id,
                    "criterion_name": criterion.name,
                    "test_weight": test_weight,
                    "original_weight": original_weight,
                    "base_recommendation": base_recommendation,
                    "test_recommendation": test_recommendation,
                    "recommendation_changed": recommendation_changed,
                    "score_changes": {
                        opt_id: test_totals.get(opt_id, 0.0) - base_totals.get(opt_id, 0.0)
                        for opt_id in set(base_totals.keys()) | set(test_totals.keys())
                    }
                }

        # Calculate overall stability
        total_tests = len(sensitivity_results)
        changed_tests = sum(1 for v in sensitivity_results.values() if v["recommendation_changed"])
        stability_score = 1.0 - (changed_tests / total_tests) if total_tests > 0 else 1.0

        return {
            "base_recommendation": base_recommendation,
            "base_totals": base_totals,
            "sensitivity_tests": sensitivity_results,
            "stability_score": stability_score,
            "interpretation": "High stability score (>0.8) means recommendation is robust to weight changes"
        }

    async def _generate_recommendation(
        self,
        options: List[DecisionOption],
        criteria: List[DecisionCriterion],
        weighted_totals: Dict[str, float],
        trade_offs: List[Dict[str, Any]]
    ) -> tuple[Optional[str], Optional[RecommendationStrength], float]:
        """
        Generate a recommendation based on weighted scores and trade-offs.

        Returns:
            Tuple of (recommended_option_id, recommendation_strength, confidence)
        """
        if not weighted_totals:
            return None, None, 0.0

        # Sort options by weighted total (descending)
        sorted_options = sorted(weighted_totals.items(), key=lambda x: x[1], reverse=True)

        best_option_id, best_score = sorted_options[0]
        second_best_score = sorted_options[1][1] if len(sorted_options) > 1 else 0.0

        # Calculate confidence based on score gap and trade-offs
        score_gap = best_score - second_best_score
        max_possible_score = 1.0  # Since weights sum to 1.0 and scores are 0-1
        relative_gap = score_gap / max_possible_score if max_possible_score > 0 else 0.0

        # Base confidence from score gap
        gap_confidence = min(0.9, 0.5 + relative_gap * 0.8)  # Maps 0 gap to 0.5, large gap to 0.9

        # Adjust confidence based on trade-offs complexity
        trade_off_penalty = min(0.2, len(trade_offs) * 0.02)  # More trade-offs = less confidence
        confidence = max(0.3, gap_confidence - trade_off_penalty)

        # Determine recommendation strength
        if relative_gap >= 0.3:
            strength = RecommendationStrength.VERY_STRONG
        elif relative_gap >= 0.2:
            strength = RecommendationStrength.STRONG
        elif relative_gap >= 0.1:
            strength = RecommendationStrength.MODERATE
        else:
            strength = RecommendationStrength.WEAK

        # Override to weak if confidence is low
        if confidence < 0.5:
            strength = RecommendationStrength.WEAK

        return best_option_id, strength, confidence

    async def create_decision_option(
        self,
        title: str,
        description: str,
        criterion_scores: Optional[Dict[str, float]] = None,
        pros: Optional[List[str]] = None,
        cons: Optional[List[str]] = None,
        risks: Optional[List[str]] = None,
        assumptions: Optional[List[str]] = None
    ) -> DecisionOption:
        """
        Create a decision option.

        Args:
            title: Option title
            description: Option description
            criterion_scores: Dictionary of criterion_id -> score (0.0 to 1.0)
            pros: List of advantages
            cons: List of disadvantages
            risks: List of risks
            assumptions: List of assumptions

        Returns:
            Created DecisionOption
        """
        option_id = f"opt_{len(self.options) + 1:03d}" if hasattr(self, 'options') else "opt_001"
        # In a real system, we'd have a central ID generator, but for simplicity:
        import random
        option_id = f"opt_{random.randint(100, 999)}"

        option = DecisionOption(
            id=option_id,
            title=title,
            description=description,
            scores=criterion_scores or {},
            pros=pros or [],
            cons=cons or [],
            risks=risks or [],
            assumptions=assumptions or []
        )

        logger.debug(f"Created decision option: {option_id}")
        return option

    async def create_decision_criterion(
        self,
        name: str,
        description: str,
        weight: float,
        is_benefit: bool = True
    ) -> DecisionCriterion:
        """
        Create a decision criterion.

        Args:
            name: Criterion name
            description: Criterion description
            weight: Relative weight (will be normalized during analysis)
            is_benefit: True if higher score is better, False if lower is better

        Returns:
            Created DecisionCriterion
        """
        criterion_id = f"crit_{random.randint(100, 999)}"
        criterion = DecisionCriterion(
            id=criterion_id,
            name=name,
            description=description,
            weight=weight,
            is_benefit=is_benefit
        )

        logger.debug(f"Created decision criterion: {criterion_id}")
        return criterion

    async def explain_recommendation(
        self,
        analysis: DecisionAnalysis
    ) -> Dict[str, Any]:
        """
        Generate an explanation for the recommendation.

        Args:
            analysis: DecisionAnalysis object

        Returns:
            Dictionary containing explanation components
        """
        if not analysis.recommended_option_id:
            return {"error": "No recommendation available"}

        # Find the recommended option
        recommended_option = None
        for option in analysis.options:
            if option.id == analysis.recommended_option_id:
                recommended_option = option
                break

        if not recommended_option:
            return {"error": "Recommended option not found"}

        # Get the option's detailed scores
        option_scores = analysis.detailed_scores.get(recommended_option.id, {})

        # Calculate contribution of each criterion to the total score
        criterion_contributions = {}
        total_score = analysis.weighted_totals.get(recommended_option.id, 0.0)

        for criterion in analysis.criteria:
            crit_id = criterion.id
            weighted_score = option_scores.get(crit_id, 0.0)
            contribution_pct = (weighted_score / total_score * 100) if total_score > 0 else 0.0
            criterion_contributions[crit_id] = {
                "criterion_name": criterion.name,
                "raw_score": recommended_option.scores.get(crit_id, 0.0),
                "weight": criterion.weight_normalized,
                "weighted_score": weighted_score,
                "contribution_percent": round(contribution_pct, 1)
            }

        # Identify key pros and cons
        key_pros = recommended_option.pros[:3]  # Top 3 pros
        key_cons = recommended_option.cons[:3]   # Top 3 cons

        # Identify main trade-offs involving this option
        related_trade_offs = []
        for trade_off in analysis.trade_offs:
            if (trade_off["option_a_id"] == recommended_option.id or
                trade_off["option_b_id"] == recommended_option.id):
                related_trade_offs.append(trade_off)

        explanation = {
            "recommended_option": {
                "id": recommended_option.id,
                "title": recommended_option.title,
                "description": recommended_option.description
            },
            "recommendation_strength": analysis.recommendation_strength.value if analysis.recommendation_strength else None,
            "confidence": analysis.confidence,
            "total_score": round(total_score, 3),
            "criterion_contributions": criterion_contributions,
            "key_pros": key_pros,
            "key_cons": key_cons,
            "related_trade_offs": related_trade_offs,
            "summary_explanation": await self._generate_summary_explanation(
                analysis, recommended_option, criterion_contributions, key_pros, key_cons
            )
        }

        return explanation

    async def _generate_summary_explanation(
        self,
        analysis: DecisionAnalysis,
        option: DecisionOption,
        contributions: Dict[str, Any],
        pros: List[str],
        cons: List[str]
    ) -> str:
        """Generate a natural language explanation of the recommendation."""
        explanation_parts = []

        # Opening statement
        explanation_parts.append(
            f"I recommend '{option.title}' as the best choice for '{analysis.decision_title}'."
        )

        # Reason based on scores
        if analysis.recommendation_strength:
            strength_text = {
                RecommendationStrength.WEAK: "a slight preference",
                RecommendationStrength.MODERATE: "a moderate preference",
                RecommendationStrength.STRONG: "a strong preference",
                RecommendationStrength.VERY_STRONG: "a very strong preference"
            }.get(analysis.recommendation_strength, "a preference")
            explanation_parts.append(
                f"This is based on {strength_text} (confidence: {analysis.confidence:.0%}) "
                f"after evaluating {len(analysis.options)} options against {len(analysis.criteria)} criteria."
            )

        # Top contributing criteria
        if contributions:
            sorted_contribs = sorted(
                contributions.items(),
                key=lambda x: x[1]["contribution_percent"],
                reverse=True
            )
            top_criteria = sorted_contribs[:2]  # Top 2 contributors
            if top_criteria:
                criteria_text = ", ".join([
                    f"'{v['criterion_name']}' ({v['contribution_percent']:.0f}%)"
                    for k, v in top_criteria
                ])
                explanation_parts.append(
                    f"The highest contributing criteria were {criteria_text}."
                )

        # Key pros
        if pros:
            explanation_parts.append(f"Key advantages include: {'; '.join(pros)}.")

        # Key cons (if any)
        if cons:
            explanation_parts.append(f"Main drawbacks to consider: {'; '.join(cons)}.")

        # Closing
        explanation_parts.append(
            f"Overall, this option provides the best balance of benefits and drawbacks "
            f"given your stated priorities and constraints."
        )

        return " ".join(explanation_parts)


# Example usage
if __name__ == "__main__":
    import asyncio

    async def main():
        # Initialize decision analyzer
        analyzer = DecisionAnalyzer()

        # Create criteria for a job offer decision
        criteria = [
            await analyzer.create_decision_criterion(
                name="Salary",
                description="Annual compensation package",
                weight=0.25,
                is_benefit=True
            ),
            await analyzer.create_decision_criterion(
                name="Work-Life Balance",
                description="Flexibility, remote work options, and hours",
                weight=0.20,
                is_benefit=True
            ),
            await analyzer.create_decision_criterion(
                name="Career Growth",
                description="Opportunities for advancement and skill development",
                weight=0.20,
                is_benefit=True
            ),
            await analyzer.create_decision_criterion(
                name="Company Culture",
                description="Values, team dynamics, and work environment",
                weight=0.15,
                is_benefit=True
            ),
            await analyzer.create_decision_criterion(
                name="Commute Time",
                description="Daily travel time to work (lower is better)",
                weight=0.10,
                is_benefit=False  # Lower commute time is better
            ),
            await analyzer.create_decision_criterion(
                name="Benefits",
                description="Health insurance, retirement, and other perks",
                weight=0.10,
                is_benefit=True
            )
        ]

        # Create options
        option_a = await analyzer.create_decision_option(
            title="Tech Startup Offer",
            description="Join a fast-growing AI startup as a senior engineer",
            criterion_scores={
                "crit_100": 0.9,  # Salary: high equity + moderate base
                "crit_101": 0.7,  # Work-Life Balance: flexible but demanding
                "crit_102": 0.9,  # Career Growth: rapid growth potential
                "crit_103": 0.8,  # Company Culture: innovative but intense
                "crit_104": 0.6,  # Commute Time: 45 minutes each way
                "crit_105": 0.7   # Benefits: good but not comprehensive
            },
            pros=[
                "High upside potential with equity",
                "Cutting-edge technology work",
                "Rapid skill development",
                "Innovative and dynamic environment"
            ],
            cons=[
                "Higher risk of company failure",
                "Long hours during product launches",
                "Less structured career path",
                "Equity may not liquidate"
            ],
            risks=[
                "Market downturn affecting funding",
                "Key personnel turnover",
                "Product-market fit challenges"
            ],
            assumptions=[
                "Current salary trends continue for 2 years",
                "Remote work policy remains flexible"
            ]
        )

        option_b = await analyzer.create_decision_option(
            title="Established Corporation Offer",
            description="Join a Fortune 500 technology company as a senior engineer",
            criterion_scores={
                "crit_100": 0.8,  # Salary: high base, lower equity
                "crit_101": 0.9,  # Work-Life Balance: more predictable hours
                "crit_102": 0.7,  # Career Growth: steady but slower advancement
                "crit_103": 0.9,  # Company Culture: stable and inclusive
                "crit_104": 0.4,  # Commute Time: 75 minutes each way
                "crit_105": 0.9   # Benefits: excellent comprehensive package
            },
            pros=[
                "Job security and stable income",
                "Excellent benefits and retirement plans",
                "Clear career progression paths",
                "Prestigious resume booster"
            ],
            cons=[
                "Slower pace of innovation",
                "More bureaucratic processes",
                "Less impact per individual contribution",
                "Potential for stagnation"
            ],
            risks=[
                "Industry disruption affecting long-term relevance",
                "Organizational changes impacting role",
                "Technology stack becoming legacy"
            ],
            assumptions=[
                "Company maintains market leadership",
                "Internal transfer opportunities remain available"
            ]
        )

        option_c = await analyzer.create_decision_option(
            title="Remote Consulting Position",
            description="Work as a independent consultant for various tech clients",
            criterion_scores={
                "crit_100": 0.7,  # Salary: variable but potentially high
                "crit_101": 0.95, # Work-Life Balance: maximum flexibility
                "crit_102": 0.6,  # Career Growth: self-directed learning
                "crit_103": 0.8,  # Company Culture: varies by client
                "crit_104": 0.95, # Commute Time: eliminated (remote)
                "crit_105": 0.5   # Benefits: self-provided
            },
            pros=[
                "Maximum flexibility and autonomy",
                "Variety of projects and industries",
                "No commute time",
                "Potential for higher effective hourly rate"
            ],
            cons=[
                "Income variability and uncertainty",
                "Need to constantly seek new clients",
                "No employer-provided benefits",
                "Self-employment tax complexities"
            ],
            risks=[
                "Periods of low demand between contracts",
                "Difficulty building deep expertise in one area",
                "Loss of employer-sponsored training",
                "Challenges with work-life boundaries"
            ],
            assumptions=[
                "Able to maintain consistent client pipeline",
                "Health insurance available through marketplace"
            ]
        )

        # Analyze the decision
        decision_title = "Choose next career move"
        analysis = await analyzer.analyze_decision(
            decision_title=decision_title,
            decision_type=DecisionType.PERSONAL,
            options=[option_a, option_b, option_c],
            criteria=criteria
        )

        # Display results
        print(f"Decision Analysis: {analysis.decision_title}")
        print(f"Options Considered: {len(analysis.options)}")
        print(f"Criteria Used: {len(analysis.criteria)}")
        print()

        print("Weighted Scores:")
        for option_id, score in analysis.weighted_totals.items():
            option = next((opt for opt in analysis.options if opt.id == option_id), None)
            name = option.title if option else option_id
            print(f"  {name}: {score:.3f}")
        print()

        if analysis.recommended_option_id:
            recommended = next((opt for opt in analysis.options if opt.id == analysis.recommended_option_id), None)
            print(f"Recommended Option: {recommended.title if recommended else analysis.recommended_option_id}")
            print(f"Recommendation Strength: {analysis.recommendation_strength.value if analysis.recommendation_strength else 'None'}")
            print(f"Confidence: {analysis.confidence:.0%}")
            print()

            # Get explanation
            explanation = await analyzer.explain_recommendation(analysis)
            print("Explanation:")
            print(explanation.get("summary_explanation", "No explanation available"))
            print()

            print("Criterion Contributions for Recommended Option:")
            for crit_id, contrib in explanation.get("criterion_contributions", {}).items():
                print(f"  {contrib['criterion_name']}: {contrib['contribution_percent']:.0f}% "
                      f"(score: {contrib['raw_score']:.2f}, weight: {contrib['weight']:.2f})")
            print()

        print("Trade-offs Identified:")
        for trade_off in analysis.trade_offs:
            print(f"  {trade_off['summary']}")
        print()

        print(f"Stereotype Sensitivity Score: {analysis.sensitivity_analysis.get('stability_score', 0):.0%}")
        print(f"Assumptions: {len(analysis.assumptions)} key assumptions identified")

    asyncio.run(main())