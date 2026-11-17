"""
Task orchestration framework for NEXUS
Handles goal decomposition, planning, progress tracking, and execution
"""

from typing import Any, Dict, List, Optional
from datetime import datetime, timedelta
import asyncio
import logging
from dataclasses import dataclass, field
from enum import Enum

from ..core.memory import MemoryEntry, MemoryType, MemorySystem

logger = logging.getLogger(__name__)


class TaskStatus(Enum):
    """Status of a task or milestone"""
    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    COMPLETED = "completed"
    BLOCKED = "blocked"
    CANCELLED = "cancelled"


class Priority(Enum):
    """Priority levels for tasks"""
    LOW = "low"
    MEDIUM = "medium"
    HIGH = "high"
    CRITICAL = "critical"


@dataclass
class Task:
    """Represents a task or sub-task"""
    id: str
    title: str
    description: str
    status: TaskStatus = TaskStatus.PENDING
    priority: Priority = Priority.MEDIUM
    estimated_effort: Optional[float] = None  # in hours
    actual_effort: Optional[float] = None  # in hours
    dependencies: List[str] = field(default_factory=list)  # IDs of dependent tasks
    assignee: Optional[str] = None  # who is responsible
    due_date: Optional[datetime] = None
    created_at: datetime = field(default_factory=datetime.now)
    updated_at: datetime = field(default_factory=datetime.now)
    completed_at: Optional[datetime] = None
    tags: List[str] = field(default_factory=list)
    notes: str = ""
    progress: float = 0.0  # 0.0 to 1.0 for partial completion

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for serialization"""
        data = {
            "id": self.id,
            "title": self.title,
            "description": self.description,
            "status": self.status.value,
            "priority": self.priority.value,
            "estimated_effort": self.estimated_effort,
            "actual_effort": self.actual_effort,
            "dependencies": self.dependencies,
            "assignee": self.assignee,
            "due_date": self.due_date.isoformat() if self.due_date else None,
            "created_at": self.created_at.isoformat(),
            "updated_at": self.updated_at.isoformat(),
            "completed_at": self.completed_at.isoformat() if self.completed_at else None,
            "tags": self.tags,
            "notes": self.notes,
            "progress": self.progress
        }
        return data

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> 'Task':
        """Create from dictionary"""
        data = data.copy()
        data['status'] = TaskStatus(data['status'])
        data['priority'] = Priority(data['priority'])
        if data.get('due_date'):
            data['due_date'] = datetime.fromisoformat(data['due_date'])
        if data.get('created_at'):
            data['created_at'] = datetime.fromisoformat(data['created_at'])
        if data.get('updated_at'):
            data['updated_at'] = datetime.fromisoformat(data['updated_at'])
        if data.get('completed_at'):
            data['completed_at'] = datetime.fromisoformat(data['completed_at'])
        return cls(**data)


@dataclass
class Milestone:
    """Represents a milestone in a project"""
    id: str
    title: str
    description: str
    target_date: datetime
    tasks: List[str] = field(default_factory=list)  # IDs of related tasks
    status: TaskStatus = TaskStatus.PENDING
    progress: float = 0.0  # 0.0 to 1.0
    created_at: datetime = field(default_factory=datetime.now)
    updated_at: datetime = field(default_factory=datetime.now)


@dataclass
class Project:
    """Represents a project composed of tasks and milestones"""
    id: str
    name: str
    description: str
    tasks: Dict[str, Task] = field(default_factory=dict)
    milestones: Dict[str, Milestone] = field(default_factory=dict)
    goals: List[str] = field(default_factory=list)
    stakeholders: List[str] = field(default_factory=list)
    start_date: Optional[datetime] = None
    end_date: Optional[datetime] = None
    created_at: datetime = field(default_factory=datetime.now)
    updated_at: datetime = field(default_factory=datetime.now)
    tags: List[str] = field(default_factory=list)


class TaskOrchestrator:
    """
    Orchestrates tasks and projects for NEXUS.
    Handles goal decomposition, planning, dependency management, and progress tracking.
    """

    def __init__(self, memory_system: Optional['MemorySystem'] = None):
        self.memory_system = memory_system
        self.projects: Dict[str, Project] = {}
        self.task_counter = 0
        self.milestone_counter = 0

    def _generate_task_id(self) -> str:
        """Generate a unique task ID"""
        self.task_counter += 1
        return f"task_{self.task_counter:04d}"

    def _generate_milestone_id(self) -> str:
        """Generate a unique milestone ID"""
        self.milestone_counter += 1
        return f"milestone_{self.milestone_counter:04d}"

    async def create_task(
        self,
        title: str,
        description: str,
        project_id: Optional[str] = None,
        priority: Priority = Priority.MEDIUM,
        estimated_effort: Optional[float] = None,
        dependencies: Optional[List[str]] = None,
        assignee: Optional[str] = None,
        due_date: Optional[datetime] = None,
        tags: Optional[List[str]] = None
    ) -> Task:
        """
        Create a new task.

        Args:
            title: Task title
            description: Task description
            project_id: Optional project ID to associate with
            priority: Task priority
            estimated_effort: Estimated effort in hours
            dependencies: List of task IDs this task depends on
            assignee: Who is responsible for the task
            due_date: When the task is due
            tags: Optional tags for categorization

        Returns:
            Created Task object
        """
        task = Task(
            id=self._generate_task_id(),
            title=title,
            description=description,
            priority=priority,
            estimated_effort=estimated_effort,
            dependencies=dependencies or [],
            assignee=assignee,
            due_date=due_date,
            tags=tags or []
        )

        # Store in memory if available
        if self.memory_system:
            memory_entry = MemoryEntry(
                id=f"task_{task.id}",
                memory_type=MemoryType.PROCEDURAL,
                content=task.to_dict(),
                importance_score=0.6,
                tags=["task", project_id] if project_id else ["task"],
                expires_at=None  # Tasks are persistent until completed/deleted
            )
            await self.memory_system.store_memory(memory_entry)

        # Add to project if specified
        if project_id and project_id in self.projects:
            self.projects[project_id].tasks[task.id] = task
            self.projects[project_id].updated_at = datetime.now()

        logger.info(f"Created task {task.id}: {task.title}")
        return task

    async def create_milestone(
        self,
        title: str,
        description: str,
        target_date: datetime,
        project_id: Optional[str] = None,
        task_ids: Optional[List[str]] = None
    ) -> Milestone:
        """
        Create a new milestone.

        Args:
            title: Milestone title
            description: Milestone description
            target_date: Target completion date
            project_id: Optional project ID to associate with
            task_ids: Optional list of task IDs related to this milestone

        Returns:
            Created Milestone object
        """
        milestone = Milestone(
            id=self._generate_milestone_id(),
            title=title,
            description=description,
            target_date=target_date,
            tasks=task_ids or []
        )

        # Add to project if specified
        if project_id and project_id in self.projects:
            self.projects[project_id].milestones[milestone.id] = milestone
            self.projects[project_id].updated_at = datetime.now()

        logger.info(f"Created milestone {milestone.id}: {milestone.title}")
        return milestone

    async def create_project(
        self,
        name: str,
        description: str,
        goals: Optional[List[str]] = None,
        stakeholders: Optional[List[str]] = None,
        start_date: Optional[datetime] = None,
        end_date: Optional[datetime] = None,
        tags: Optional[List[str]] = None
    ) -> Project:
        """
        Create a new project.

        Args:
            name: Project name
            description: Project description
            goals: List of project goals
            stakeholders: List of project stakeholders
            start_date: Project start date
            end_date: Project end date
            tags: Optional tags for categorization

        Returns:
            Created Project object
        """
        project = Project(
            id=f"proj_{len(self.projects) + 1:04d}",
            name=name,
            description=description,
            goals=goals or [],
            stakeholders=stakeholders or [],
            start_date=start_date,
            end_date=end_date,
            tags=tags or []
        )

        self.projects[project.id] = project
        logger.info(f"Created project {project.id}: {project.name}")
        return project

    async def update_task_status(
        self,
        task_id: str,
        status: TaskStatus,
        progress: Optional[float] = None,
        actual_effort: Optional[float] = None,
        notes: Optional[str] = None
    ) -> bool:
        """
        Update the status of a task.

        Args:
            task_id: ID of task to update
            status: New status
            progress: Optional progress percentage (0.0 to 1.0)
            actual_effort: Optional actual effort spent (in hours)
            notes: Optional notes about the update

        Returns:
            True if updated, False if task not found
        """
        # Find the task in any project
        task = None
        project_id = None
        for pid, project in self.projects.items():
            if task_id in project.tasks:
                task = project.tasks[task_id]
                project_id = pid
                break

        if not task:
            logger.warning(f"Task {task_id} not found")
            return False

        # Update task fields
        task.status = status
        if progress is not None:
            task.progress = max(0.0, min(1.0, progress))
        if actual_effort is not None:
            task.actual_effort = actual_effort
        if notes is not None:
            task.notes = notes
        task.updated_at = datetime.now()

        if status == TaskStatus.COMPLETED and not task.completed_at:
            task.completed_at = datetime.now()
            task.progress = 1.0

        # Update project timestamp
        if project_id:
            self.projects[project_id].updated_at = datetime.now()

        # Store updated task in memory
        if self.memory_system:
            memory_entry = MemoryEntry(
                id=f"task_{task.id}",
                memory_type=MemoryType.PROCEDURAL,
                content=task.to_dict(),
                importance_score=0.7 if status == TaskStatus.COMPLETED else 0.6,
                tags=["task", project_id] if project_id else ["task"],
                expires_at=None
            )
            await self.memory_system.store_memory(memory_entry)

        logger.info(f"Updated task {task_id} status to {status.value}")
        return True

    async def get_task(self, task_id: str) -> Optional[Task]:
        """
        Get a task by ID.

        Args:
            task_id: ID of task to retrieve

        Returns:
            Task object if found, None otherwise
        """
        for project in self.projects.values():
            if task_id in project.tasks:
                return project.tasks[task_id]
        return None

    async def get_project_tasks(
        self,
        project_id: str,
        status_filter: Optional[TaskStatus] = None
    ) -> List[Task]:
        """
        Get all tasks for a project, optionally filtered by status.

        Args:
            project_id: ID of project
            status_filter: Optional status to filter by

        Returns:
            List of Task objects
        """
        if project_id not in self.projects:
            return []

        tasks = list(self.projects[project_id].tasks.values())
        if status_filter:
            tasks = [task for task in tasks if task.status == status_filter]
        return tasks

    async def get_project_milestones(
        self,
        project_id: str
    ) -> List[Milestone]:
        """
        Get all milestones for a project.

        Args:
            project_id: ID of project

        Returns:
            List of Milestone objects
        """
        if project_id not in self.projects:
            return []
        return list(self.projects[project_id].milestones.values())

    async def decompose_goal(
        self,
        goal: str,
        context: Optional[Dict[str, Any]] = None,
        timeframe: Optional[str] = None,
        resources: Optional[List[str]] = None
    ) -> Dict[str, Any]:
        """
        Decompose a high-level goal into actionable tasks and milestones.

        Args:
            goal: The high-level goal to decompose
            context: Optional contextual information
            timeframe: Optional timeframe for completion
            resources: Optional list of available resources

        Returns:
            Dictionary containing decomposition results
        """
        # This is a simplified implementation
        # In a real system, this would use more sophisticated AI planning

        logger.info(f"Decomposing goal: {goal}")

        # Simple heuristic-based decomposition
        # In reality, this would involve:
        # - Understanding the goal domain
        # - Identifying required deliverables
        # - Breaking down into work packages
        # - Identifying dependencies
        # - Estimating effort
        # - Creating a timeline

        # Create a project for this goal
        project = await self.create_project(
            name=f"Goal: {goal[:50]}{'...' if len(goal) > 50 else ''}",
            description=f"Project created to achieve goal: {goal}",
            goals=[goal],
            start_date=datetime.now()
        )

        # Generate some initial tasks based on common patterns
        initial_tasks = [
            {
                "title": "Research and analysis",
                "description": f"Gather information and analyze requirements for: {goal}",
                "estimated_effort": 2.0,
                "priority": Priority.HIGH
            },
            {
                "title": "Planning and design",
                "description": f"Create detailed plan and design for achieving: {goal}",
                "estimated_effort": 3.0,
                "priority": Priority.HIGH,
                "dependencies": []  # Will be set after creation
            },
            {
                "title": "Implementation",
                "description": f"Execute the plan to achieve: {goal}",
                "estimated_effort": 8.0,
                "priority": Priority.MEDIUM,
                "dependencies": []  # Will be set after creation
            },
            {
                "title": "Testing and validation",
                "description": f"Verify that the goal has been achieved: {goal}",
                "estimated_effort": 2.0,
                "priority": Priority.MEDIUM,
                "dependencies": []  # Will be set after creation
            },
            {
                "title": "Review and documentation",
                "description": f"Review outcomes and document lessons learned for: {goal}",
                "estimated_effort": 1.5,
                "priority": Priority.LOW,
                "dependencies": []  # Will be set after creation
            }
        ]

        # Create tasks and set up dependencies
        created_tasks = []
        for i, task_info in enumerate(initial_tasks):
            # Set dependencies: each task depends on the previous one (except first)
            deps = []
            if i > 0 and created_tasks:
                deps = [created_tasks[i-1].id]

            task = await self.create_task(
                title=task_info["title"],
                description=task_info["description"],
                project_id=project.id,
                estimated_effort=task_info["estimated_effort"],
                priority=task_info["priority"],
                dependencies=deps
            )
            created_tasks.append(task)

        # Create milestones
        milestones = [
            await self.create_milestone(
                title="Planning Complete",
                description="Finished research, analysis, and planning phase",
                target_date=datetime.now() + timedelta(days=3),
                project_id=project.id,
                task_ids=[created_tasks[0].id, created_tasks[1].id] if len(created_tasks) >= 2 else []
            ),
            await self.create_milestone(
                title="Implementation Complete",
                description="Finished implementation phase",
                target_date=datetime.now() + timedelta(days=7),
                project_id=project.id,
                task_ids=[created_tasks[2].id] if len(created_tasks) >= 3 else []
            ),
            await self.create_milestone(
                title="Goal Achieved",
                description="Successfully achieved the goal and documented outcomes",
                target_date=datetime.now() + timedelta(days=10),
                project_id=project.id,
                task_ids=[created_tasks[3].id, created_tasks[4].id] if len(created_tasks) >= 5 else []
            )
        ]

        return {
            "project_id": project.id,
            "project_name": project.name,
            "goal": goal,
            "tasks_created": len(created_tasks),
            "milestones_created": len(milestones),
            "estimated_total_effort": sum(task.estimated_effort or 0 for task in created_tasks),
            "timeline": {
                "start": project.start_date.isoformat() if project.start_date else None,
                "estimated_end": (datetime.now() + timedelta(days=10)).isoformat()
            },
            "next_steps": [
                f"Begin work on task: {created_tasks[0].title}" if created_tasks else "No tasks created"
            ]
        }

    async def get_progress_report(self, project_id: str) -> Dict[str, Any]:
        """
        Generate a progress report for a project.

        Args:
            project_id: ID of project to report on

        Returns:
            Dictionary containing progress report
        """
        if project_id not in self.projects:
            return {"error": f"Project {project_id} not found"}

        project = self.projects[project_id]
        tasks = list(project.tasks.values())
        milestones = list(project.milestones.values())

        # Calculate overall progress
        if tasks:
            task_progress = sum(task.progress for task in tasks) / len(tasks)
        else:
            task_progress = 0.0

        if milestones:
            milestone_progress = sum(milestone.progress for milestone in milestones) / len(milestones)
        else:
            milestone_progress = 0.0

        overall_progress = (task_progress + milestone_progress) / 2

        # Count tasks by status
        status_counts = {}
        for status in TaskStatus:
            status_counts[status.value] = len([t for t in tasks if t.status == status])

        # Identify blocked tasks
        blocked_tasks = [t for t in tasks if t.status == TaskStatus.BLOCKED]

        # Identify overdue tasks
        now = datetime.now()
        overdue_tasks = [
            t for t in tasks
            if t.due_date and t.due_date < now and t.status != TaskStatus.COMPLETED
        ]

        # Identify upcoming milestones
        upcoming_milestones = [
            m for m in milestones
            if m.target_date > now and m.status != TaskStatus.COMPLETED
        ]
        upcoming_milestones.sort(key=lambda m: m.target_date)

        return {
            "project_id": project.id,
            "project_name": project.name,
            "overall_progress": round(overall_progress, 3),
            "task_progress": round(task_progress, 3),
            "milestone_progress": round(milestone_progress, 3),
            "task_counts": status_counts,
            "total_tasks": len(tasks),
            "completed_tasks": status_counts.get("completed", 0),
            "blocked_tasks": [
                {
                    "id": t.id,
                    "title": t.title,
                    "blocked_since": t.updated_at.isoformat()
                }
                for t in blocked_tasks
            ],
            "overdue_tasks": [
                {
                    "id": t.id,
                    "title": t.title,
                    "due_date": t.due_date.isoformat(),
                    "days_overdue": (now - t.due_date).days
                }
                for t in overdue_tasks
            ],
            "upcoming_milestones": [
                {
                    "id": m.id,
                    "title": m.title,
                    "target_date": m.target_date.isoformat(),
                    "progress": m.progress
                }
                for m in upcoming_milestones[:5]  # Limit to next 5
            ],
            "last_updated": project.updated_at.isoformat()
        }

    async def suggest_next_actions(
        self,
        project_id: str,
        limit: int = 5
    ) -> List[Dict[str, Any]]:
        """
        Suggest next actions based on current project state.

        Args:
            project_id: ID of project
            limit: Maximum number of suggestions to return

        Returns:
            List of suggested next actions
        """
        if project_id not in self.projects:
            return [{"error": f"Project {project_id} not found"}]

        project = self.projects[project_id]
        suggestions = []

        # Find ready tasks (dependencies completed, not blocked/in progress)
        ready_tasks = []
        for task in project.tasks.values():
            if task.status == TaskStatus.PENDING:
                # Check if all dependencies are completed
                deps_completed = True
                for dep_id in task.dependencies:
                    dep_task = project.tasks.get(dep_id)
                    if not dep_task or dep_task.status != TaskStatus.COMPLETED:
                        deps_completed = False
                        break
                if deps_completed:
                    ready_tasks.append(task)

        # Sort by priority and estimated effort
        priority_order = {Priority.CRITICAL: 0, Priority.HIGH: 1, Priority.MEDIUM: 2, Priority.LOW: 3}
        ready_tasks.sort(key=lambda t: (priority_order[t.priority], t.estimated_effort or 0))

        # Add ready tasks as suggestions
        for task in ready_tasks[:limit]:
            suggestions.append({
                "type": "task",
                "id": task.id,
                "title": task.title,
                "description": task.description,
                "priority": task.priority.value,
                "estimated_effort": task.estimated_effort,
                "reason": "Dependencies completed, ready to start"
            })

        # If we need more suggestions, look at blocked tasks
        if len(suggestions) < limit:
            blocked_tasks = [t for t in project.tasks.values() if t.status == TaskStatus.BLOCKED]
            for task in blocked_tasks[:(limit - len(suggestions))]:
                suggestions.append({
                    "type": "blocked_task",
                    "id": task.id,
                    "title": task.title,
                    "description": task.description,
                    "reason": "Task is blocked - need to resolve dependencies or issues"
                })

        # If we still need more suggestions, look at in-progress tasks
        if len(suggestions) < limit:
            in_progress_tasks = [t for t in project.tasks.values() if t.status == TaskStatus.IN_PROGRESS]
            for task in in_progress_tasks[:(limit - len(suggestions))]:
                suggestions.append({
                    "type": "in_progress_task",
                    "id": task.id,
                    "title": task.title,
                    "description": task.description,
                    "progress": task.progress,
                    "reason": "Task is in progress - consider continuing work"
                })

        return suggestions[:limit]


# Example usage
if __name__ == "__main__":
    import asyncio

    async def main():
        # Initialize orchestrator
        orchestrator = TaskOrchestrator()

        # Create a project
        project = await orchestrator.create_project(
            name="Learn Spanish",
            description="Project to learn Spanish language to B2 level",
            goals=["Achieve B2 level in Spanish", "Be able to hold conversations with native speakers"],
            stakeholders=["Self"]
        )
        print(f"Created project: {project.name} (ID: {project.id})")

        # Decompose a goal
        goal_result = await orchestrator.decompose_goal(
            goal="Prepare a presentation on renewable energy for tomorrow's meeting",
            timeframe="1 day",
            resources=["Computer", "Internet", "Presentation software"]
        )
        print(f"\nGoal decomposition result:")
        print(f"Project ID: {goal_result['project_id']}")
        print(f"Tasks created: {goal_result['tasks_created']}")
        print(f"Estimated effort: {goal_result['estimated_total_effort']} hours")
        print(f"Next steps: {goal_result['next_steps']}")

        # Get progress report
        progress = await orchestrator.get_progress_report(goal_result['project_id'])
        print(f"\nProgress report:")
        print(f"Overall progress: {progress['overall_progress']}")
        print(f"Task counts: {progress['task_counts']}")

        # Suggest next actions
        suggestions = await orchestrator.suggest_next_actions(goal_result['project_id'])
        print(f"\nSuggested next actions:")
        for i, suggestion in enumerate(suggestions, 1):
            print(f"{i}. {suggestion['title']} ({suggestion['type']})")

        # Update a task status
        if goal_result['tasks_created'] > 0:
            # Get the first task ID
            tasks = await orchestrator.get_project_tasks(goal_result['project_id'])
            if tasks:
                first_task_id = tasks[0].id
                await orchestrator.update_task_status(
                    task_id=first_task_id,
                    status=TaskStatus.IN_PROGRESS,
                    progress=0.3,
                    notes="Started research, found some good sources"
                )
                print(f"\nUpdated task {first_task_id} to IN_PROGRESS")

                # Check progress again
                progress2 = await orchestrator.get_progress_report(goal_result['project_id'])
                print(f"Updated progress: {progress2['overall_progress']}")

    asyncio.run(main())