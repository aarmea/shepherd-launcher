Agents: please use the existing documentation for setup.

<CONTRIBUTING.md> describes environment setup and build, test, and lint, including helper scripts and exact commands.

Please ensure that your changes build and pass tests and lint. If you changed the example configuration at <config.example.toml>, make sure that it passes config validation.

Each of the Rust crates in <crates> contains a README.md that describes each at a high level.

<.github/workflows/ci.yml> and <docs/INSTALL.md> describes exact environment setup, especially if coming from Ubuntu 24.04 (shepherd-launcher requires 25.10).

Historical prompts and design docs provided to agents are placed in <docs/ai/history>. Please refer there for history, and if this prompt is substantial, write it along with any relevant context (like the GitHub issue) to that directory as well.
