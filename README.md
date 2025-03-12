# Borg - Autonomous Self-Improving AI Agent

Borg is an autonomous self-improving AI agent implemented in Rust. It's designed to iteratively generate, modify, and evaluate its own code to improve efficiency, ensure survival, and scale over time.

## Core Components

- **Code Generation & Modification Engine**: AI-powered module that writes new Rust code or refactors existing code
- **Evaluation & Testing Harness**: Subsystem that executes generated code changes in a sandbox
- **Self-Verification Mechanisms**: Multiple layers of verification to ensure the agent does not introduce regressions
- **Resource Awareness & Survival Strategy**: Monitors system resources and prioritizes changes that improve efficiency
- **Version Control & Autonomous GitOps**: Tracks all code modifications with Git
- **Strategic Planning System**: Manages long-term goals and translates them into actionable milestones and tactical goals
- **Multi-Modal Action Framework**: Enables the agent to interact with external systems through APIs, web research, and system commands

## Agent Architecture and Workflow

The Borg agent follows a sophisticated multi-phase workflow to achieve autonomous self-improvement and goal fulfillment. The diagram below shows the agent's core execution flow:

```mermaid
flowchart TD
    Start[Agent Initialization] --> Config[Load Configuration]
    Config --> Initialize[Initialize Components]
    Initialize --> LoadGoals[Load Goals & Objectives]
    LoadGoals --> ResourceCheck[Check System Resources]

    ResourceCheck -- Resources Available --> SelectGoal[Select Next Goal]
    ResourceCheck -- Resources Low --> Wait[Wait and Retry]
    Wait --> ResourceCheck

    SelectGoal -- Goal Found --> StrategySelection[Select Strategy]
    SelectGoal -- No Goals --> End[Idle: Wait for Goals]

    StrategySelection --> PlanCreation[Create Execution Plan]
    PlanCreation --> EthicsCheck[Ethical Assessment]

    EthicsCheck -- Pass --> Execute[Execute Plan]
    EthicsCheck -- Fail --> MarkFailed[Mark Goal as Failed]
    MarkFailed --> SaveGoals[Save Goals to Disk]

    Execute --> Evaluation[Evaluate Results]

    Evaluation -- Success --> MarkCompleted[Mark Goal as Completed]
    Evaluation -- Failure --> MarkFailed

    MarkCompleted --> SaveGoals
    SaveGoals --> ResourceCheck
```

### Key Phases

The agent's workflow consists of several key phases:

1. **Initialization Phase**
   - Load configuration settings
   - Initialize all core components
   - Set up the strategic planning system
   - Register available strategies
   - Load existing goals and objectives from disk

2. **Strategy Selection Phase**
   - Select next goal based on priority and dependencies
   - Evaluate applicable strategies for the goal
   - Choose the best strategy based on relevance scores
   - Verify required permissions for the selected strategy

3. **Planning Phase**
   - Create a detailed execution plan with concrete steps
   - Define step dependencies and parameters
   - Estimate resource requirements
   - Perform ethical assessment of the plan

4. **Execution Phase**
   - Execute each step of the plan in order
   - Track step dependencies and success criteria
   - Collect outputs and metrics for evaluation
   - Pause for confirmation on critical steps

5. **Evaluation Phase**
   - Analyze execution results against goal criteria
   - Run tests to validate changes
   - Update goal status based on results
   - Save progress to disk for persistence

6. **Strategy-Specific Flows**

   **Code Improvement Strategy:**
   ```mermaid
   flowchart TD
       Start[Start Code Improvement] --> AnalyzeGoal[Analyze Goal Requirements]
       AnalyzeGoal --> InitContext[Initialize LLM Context]

       %% Iterative code generation loop
       subgraph IterativeGeneration["Iterative Code Generation Phase"]
           InitContext --> ToolExploration[Explore Codebase Using Tools]
           ToolExploration --> CodeGen[Generate Code Changes]
           CodeGen --> FileUpdates[Update Files]
           FileUpdates --> EvaluateChanges[Evaluate Quality]
           EvaluateChanges --> NeedMore{More Changes<br>Needed?}
           NeedMore -- Yes --> ToolExploration
           NeedMore -- No --> SignalCompletion[Signal Implementation Complete]
       end

       SignalCompletion --> PrepareChanges[Prepare Final Changes]
       PrepareChanges --> CommitToFeatureBranch[Apply Changes to Branch]
       CommitToFeatureBranch --> RunTests[Run Tests]
       RunTests -- Tests Pass --> EvalBenchmarks[Evaluate Benchmarks]
       RunTests -- Tests Fail --> MarkFailed[Mark as Failed]
       EvalBenchmarks -- Meets Criteria --> MergeChanges[Merge Changes]
       EvalBenchmarks -- Fails Criteria --> MarkFailed
       MergeChanges --> MarkCompleted[Mark as Completed]
   ```

   **API Client Strategy:**
   ```mermaid
   flowchart TD
       Start[Start API Interaction] --> Prepare[Prepare Request]
       Prepare --> PermCheck[Permission Check]
       PermCheck -- Allowed --> Execute[Execute API Call]
       PermCheck -- Denied --> Failed[Mark as Failed]
       Execute --> ParseResponse[Parse Response]
       ParseResponse --> ProcessData[Process Data]
       ProcessData --> Complete[Mark as Completed]
   ```

   **Web Research Strategy:**
   ```mermaid
   flowchart TD
       Start[Start Web Research] --> Query[Generate Search Query]
       Query --> Search[Execute Search]
       Search --> FilterResults[Filter Relevant Results]
       FilterResults --> ExtractInfo[Extract Information]
       ExtractInfo --> Synthesize[Synthesize Findings]
       Synthesize --> Integrate[Integrate into Knowledge Base]
       Integrate --> Complete[Mark as Completed]
   ```

7. **Error Recovery Phase**
   - Detect execution failures
   - Roll back changes if necessary
   - Log detailed error information
   - Update goal status

## Multi-Modal Action Framework

The agent uses a flexible strategy-based system to handle different types of actions:

- **Code Improvement**: Generates, tests, and applies code changes
- **API Interaction**: Makes calls to external services
- **Web Research**: Gathers information from the internet
- **System Commands**: Executes commands on the host system
- **Data Analysis**: Processes and analyzes structured data

Each strategy implements the same core interface while providing specialized functionality for its domain, allowing the agent to select the most appropriate approach based on the goal's requirements.

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

### Running the Agent

Borg provides a unified command-line interface with various commands:

```
# Run the main agent in autonomous mode
cargo run

# Show information about the agent
cargo run -- info

# Run a single improvement iteration
cargo run -- improve

# List all strategic objectives
cargo run -- objective list

# Add a new strategic objective
cargo run -- objective add <ID> <TITLE> <DESCRIPTION> <TIMEFRAME> <CREATOR>

# Load objectives from a TOML file
cargo run -- load-objectives examples/strategic_objective.toml

# Generate milestones and tactical goals
cargo run -- plan generate

# Show the current strategic plan
cargo run -- plan show

# Generate a progress report
cargo run -- plan report
```

For advanced usage, you can also build the binary and use it directly:

```
cargo build --release
./target/release/borg [COMMAND]
```

## Configuration

The application uses configuration files to manage its settings. For security reasons, your personal configuration with API keys is kept in a separate file that is not committed to the repository.

### Setting Up Configuration

1. The repository includes `config.toml`, which is a template file with placeholder values.

2. For local development, copy this template to a production configuration file:
   ```
   cp config.toml config.production.toml
   ```

3. Edit the `config.production.toml` file and add your API keys:
   ```toml
   # Default LLM configuration
   [llm.default]
   provider = "openai"
   api_key = "your_actual_api_key_here"
   model = "gpt-4o"

   # Additional LLM configurations for specific tasks...
   ```

4. The `config.production.toml` file is automatically ignored by Git to prevent accidentally committing your API keys.

5. By default, the application will use `config.production.toml` if it exists, and fall back to `config.toml` otherwise.

### Multi-LLM Configuration

Borg supports using different LLM providers and models for different tasks:

- **Code Generation**: Used for writing and modifying code
- **Ethics Assessment**: Evaluates ethical implications of changes
- **Planning**: High-level decision making and goal selection
- **Code Review**: Validates and reviews generated code

Each can be configured with different providers, models, and settings in the config file.

## Using the Strategic Planning System

Borg includes a comprehensive strategic planning system that allows creators to define long-term objectives and have the agent autonomously work toward them.

### Defining Strategic Objectives

Strategic objectives can be defined in TOML format:

```toml
[[objectives]]
id = "performance-2025"
title = "Optimize System Performance"
description = "Reduce the system's resource footprint by 50% while maintaining throughput."
timeframe = 12  # months
creator = "lead-developer"
key_results = [
  "Reduce memory usage by 50% from baseline",
  "Reduce CPU utilization by 40% from baseline"
]
constraints = [
  "Must maintain all existing functionality",
  "Cannot reduce security measures"
]
```

### Loading Objectives

Use the provided command to load objectives:

```
cargo run -- load-objectives examples/strategic_objective.toml
```

### Managing the Planning System

The CLI provides commands for working with the planning system:

```
# Generate milestones and tactical goals from objectives
cargo run -- plan generate

# View the current strategic plan
cargo run -- plan show

# Generate a progress report
cargo run -- plan report
```

### How It Works

The strategic planning system operates on three levels:

1. **Strategic Objectives**: Long-term goals defined by creators (6-18 months)
2. **Milestones**: Medium-term achievements that mark progress toward objectives (1-3 months)
3. **Tactical Goals**: Short-term, actionable improvements that contribute to milestones (days to weeks)

The agent automatically generates milestones from objectives and tactical goals from milestones, creating a coherent hierarchy of improvements that align with the creator's strategic vision.

## Iterative Code Generation Process

One of the most powerful aspects of the Borg agent is its iterative code generation process. Unlike traditional one-shot code generation, Borg's LLM can engage in a multi-step conversation with the codebase, using tools to explore and modify files multiple times before finalizing changes.

```mermaid
flowchart TD
    Start[Start Code Generation] --> ExtractContext[Extract Context from Goal]
    ExtractContext --> InitSession[Initialize LLM Session]

    subgraph ToolLoop["Tool Usage Loop"]
        InitSession --> PromptLLM[Prompt LLM for Next Action]
        PromptLLM --> ToolCall{Tool Call Type?}

        ToolCall -- Search --> CodeSearch[Search Codebase]
        ToolCall -- Read --> ReadFile[Read File Contents]
        ToolCall -- FindTests --> FindTests[Find Related Tests]
        ToolCall -- Explore --> ExploreDirectory[Explore Directory Structure]
        ToolCall -- GitHistory --> ViewHistory[View Git History]
        ToolCall -- Compile --> CheckCompilation[Check Compilation]

        CodeSearch --> ProcessResults[Process Tool Results]
        ReadFile --> ProcessResults
        FindTests --> ProcessResults
        ExploreDirectory --> ProcessResults
        ViewHistory --> ProcessResults
        CheckCompilation --> ProcessResults

        ProcessResults --> UpdateContext[Update LLM Context]
        UpdateContext --> PromptLLM
    end

    ToolCall -- Edit --> CodeEdit[Edit File]
    CodeEdit --> EditType{Edit Type?}
    EditType -- Intermediate --> UpdateContext
    EditType -- Final --> FinalEdits[Finalize Edits]

    FinalEdits --> CompileCheck[Compilation Check]
    CompileCheck -- Fails --> PromptLLM
    CompileCheck -- Passes --> PrepareChanges[Prepare Final Changes]

    PrepareChanges --> CommitToBranch[Commit to Feature Branch]
    CommitToBranch --> End[Proceed to Testing]
```

### Key Features of the Iterative Process:

1. **Exploration Phase**
   - The LLM uses tools to understand the codebase context
   - Code search for patterns or symbols
   - File content reading for detailed understanding
   - Test discovery to understand validation requirements
   - Directory exploration to grasp project structure
   - Git history analysis to understand code evolution

2. **Multi-tool Support**
   - The LLM can make multiple tool calls in a single response
   - Batch related operations together for efficiency
   - Example: searching multiple files, reading multiple files, or creating/modifying multiple files
   - Reduces the number of interaction cycles needed for complex tasks

3. **Code Generation Phase**
   - After gathering context, the LLM decides which files to create or modify
   - Uses create_file and modify_file tools to implement changes
   - Autonomously determines the best files to modify based on codebase understanding
   - No explicit instruction needed about which files to change

4. **Iterative Modification**
   - Multiple file edits can happen within a single generation session
   - Intermediate edits update the LLM's context
   - Continuous compilation checks provide immediate feedback
   - Each modification builds on previous ones

5. **Finalization**
   - The LLM explicitly signals when implementation is complete
   - Final compilation check ensures code validity
   - Only after completion are changes prepared for branch commit
   - The agent creates a clean branch with all changes applied

6. **Feedback Integration**
   - Results from compilation checks feed back into the LLM's context
   - Test results and benchmark results can trigger another iteration
   - The LLM learns from failures and adapts its approach

This iterative approach mirrors how human developers work, allowing the agent to build up solutions incrementally with continuous feedback, rather than attempting to generate a perfect solution in one pass.

## License

[MIT License](LICENSE)