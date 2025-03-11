# Borg - Autonomous Self-Improving AI Agent

Borg is an autonomous self-improving AI agent implemented in Rust. It's designed to iteratively generate, modify, and evaluate its own code to improve efficiency, ensure survival, and scale over time.

## Core Components

- **Code Generation & Modification Engine**: AI-powered module that writes new Rust code or refactors existing code
- **Evaluation & Testing Harness**: Subsystem that executes generated code changes in a sandbox
- **Self-Verification Mechanisms**: Multiple layers of verification to ensure the agent does not introduce regressions
- **Resource Awareness & Survival Strategy**: Monitors system resources and prioritizes changes that improve efficiency
- **Version Control & Autonomous GitOps**: Tracks all code modifications with Git

## Development Roadmap

This project follows a phased development approach:

1. **Bootstrap & Initial Self-Improvement**: Create minimal viable autonomous developer
2. **Swarm-Based Parallel Development**: Enable multiple concurrent self-improvements
3. **Conflict Resolution and Evolutionary Selection**: Implement branch competition and selection

## Getting Started

### Prerequisites

- Rust (1.70+ recommended)
- Git
- Access to an LLM API (e.g., OpenAI, Anthropic)

### Installation

1. Clone this repository:
   ```
   git clone https://github.com/yourusername/borg.git
   cd borg
   ```

2. Build the project:
   ```
   cargo build
   ```

3. Configure the agent (see Configuration section below)

4. Run the agent:
   ```
   cargo run
   ```

## Configuration

The application uses a `config.toml` file for configuration. For security reasons, this file is not committed to the repository.

### Setting Up Configuration

1. Copy the template configuration file:
   ```
   cp config.template.toml config.toml
   ```

2. Edit the `config.toml` file and add your API keys:
   ```toml
   # Default LLM configuration
   [llm.default]
   provider = "openai"
   api_key = "your_actual_api_key_here"
   model = "gpt-4o"

   # Additional LLM configurations for specific tasks...
   ```

3. The `config.toml` file is automatically ignored by Git to prevent accidentally committing your API keys.

### Multi-LLM Configuration

Borg supports using different LLM providers and models for different tasks:

- **Code Generation**: Used for writing and modifying code
- **Ethics Assessment**: Evaluates ethical implications of changes
- **Planning**: High-level decision making and goal selection
- **Code Review**: Validates and reviews generated code

Each can be configured with different providers, models, and settings in the config file.

## License

[MIT License](LICENSE)