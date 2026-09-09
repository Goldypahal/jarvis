from setuptools import setup, find_packages

with open("README.md", "r", encoding="utf-8") as fh:
    long_description = fh.read()

setup(
    name="nexus-assistant",
    version="0.1.0",
    author="NEXUS Project",
    author_email="nexus@example.com",
    description="Advanced AI Personal Assistant System",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/yourusername/nexus-assistant",
    packages=find_packages(),
    classifiers=[
        "Development Status :: 3 - Alpha",
        "Intended Audience :: End Users/Desktop",
        "License :: OSI Approved :: MIT License",
        "Operating System :: OS Independent",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
    ],
    python_requires=">=3.8",
    install_requires=[
        "typing_extensions>=4.0.0",
        "pydantic>=2.0.0",
        "fastapi>=0.100.0",
        "uvicorn>=0.20.0",
        "spacy>=3.0.0",
        "nltk>=3.0.0",
        "transformers>=4.0.0",
        "sentence-transformers>=2.0.0",
        "redis>=4.0.0",
        "python-dotenv>=0.20.0",
        "requests>=2.25.0",
        "aiohttp>=3.0.0",
    ],
    extras_require={
        "dev": [
            "pytest>=6.0.0",
            "black>=22.0.0",
            "flake8>=4.0.0",
            "mypy>=0.9.0",
        ],
        "multimodal": [
            "opencv-python>=4.0.0",
            "pillow>=8.0.0",
            "pyaudio>=0.2.0",
        ],
    },
    entry_points={
        "console_scripts": [
            "nexus=nexus.__main__:main",
        ],
    },
)