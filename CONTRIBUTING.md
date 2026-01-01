# For developers

## Build for development

### tl;dr

You need a Wayland-capable Linux system, Rust, and a small set of system
dependencies. Once installed, `./run-dev` will start a development instance.

### Requirements

1. **Linux with Wayland**

   * Any modern Wayland compositor is sufficient. For Ubuntu, this means 25.10 or higher.
   * Optional (but recommended for realistic testing): TPM-based full disk encryption and a BIOS/UEFI password to prevent local tampering.

2. **System dependencies**

   * Platform-specific packages are required for building and running.
   * View packages with: `./scripts/shepherd deps print dev`
   * Install all dev dependencies: `./scripts/shepherd deps install dev`
   * **Note**: Rust is automatically installed via rustup when installing build or dev dependencies.

### Unified script system

`shepherd-launcher` provides a unified script system for managing dependencies, building, and running:

```sh
# View and install dependencies
./scripts/shepherd deps print dev        # List all dev dependencies
./scripts/shepherd deps install dev      # Install all dev dependencies

# Build binaries
./scripts/shepherd build                 # Debug build
./scripts/shepherd build --release       # Release build

# Development
./scripts/shepherd dev run               # Build and run in nested Sway
```

For CI/build-only environments:
```sh
./scripts/shepherd deps install build    # Build dependencies only
./scripts/shepherd build --release       # Production build
```

For runtime-only systems:
```sh
./scripts/shepherd deps install run      # Runtime dependencies only
```

See `./scripts/shepherd --help` for all available commands.

### Running in development

Start a development instance:

```sh
./run-dev
```

#### Adjusting the time

To avoid having to adjust the system clock or wait for timeouts, development
builds can mock the time with `SHEPHERD_MOCK_TIME`:

```sh
SHEPHERD_MOCK_TIME="2025-12-25 15:30:00" ./run-dev
```

Time and activity history are maintained in a SQLite database, which
`./run-dev` places at `./dev-runtime/data/shepherdd.db`. Edit this database
using a tool like [DB Browser for SQLite](https://sqlitebrowser.org/) while the
service is not running to inject application usage. The schema is defined in
the [shepherd-store crate](./crates/shepherd-store/).

### Testing and linting

Run the test suite:

```sh
cargo test
# as run in CI:
cargo test --all-targets
```

Run lint checks:

```sh
cargo clippy
# as run in CI:
cargo clippy --all-targets -- -D warnings
```


## Contribution guidelines

`shepherd-launcher` is licensed under the GPLv3 to preserve end-users' rights.
By submitting a pull request, you agree to license your contributions under the
GPLv3.

Contributions written in whole or in part by generative AI are allowed;
however, they will be reviewed as if you personally authored them. I highly
recommend adding substantial prompts and design docs provided to agents to
[docs/ai/history/](./docs/ai/history/) along with the PRs and commit hashes
associated with them.

The authors of `shepherd-launcher` do not condone software or media piracy.
Contributions that explicitly promote or facilitate piracy will be rejected.
Please support developers and creators by obtaining content legally.
