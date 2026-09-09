"""
NEXUS Main Entry Point
Launches the console interface by default
"""

import asyncio
import sys
from .interfaces.console import main

if __name__ == "__main__":
    # Run the console interface
    asyncio.run(main())