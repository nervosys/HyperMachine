# Code Style

HyperMachine follows standard Rust conventions with some project-specific guidelines.

## Formatting

```bash
# Format code
cargo fmt

# Check formatting
cargo fmt --check
```

## Linting

```bash
# Run clippy
cargo clippy --workspace -- -D warnings

# Fix auto-fixable issues
cargo clippy --fix
```

## Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| Crates | `kebab-case` | `hv2-core` |
| Modules | `snake_case` | `memory_manager` |
| Types | `PascalCase` | `VirtualMachine` |
| Functions | `snake_case` | `create_vm` |
| Constants | `SCREAMING_SNAKE` | `MAX_CPU_CORES` |

## Error Handling

Use `thiserror` for error types:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VmError {
    #[error("VM not found: {0}")]
    NotFound(String),
    
    #[error("VM already running")]
    AlreadyRunning,
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

## Documentation

All public items must have documentation:

```rust
/// Creates a new virtual machine with the specified configuration.
///
/// # Arguments
///
/// * `config` - The VM configuration
///
/// # Returns
///
/// The created VM handle, or an error if creation failed.
///
/// # Example
///
/// ```
/// let vm = Vm::create(VmConfig {
///     name: "test".into(),
///     cpu_cores: 4,
///     memory_mb: 8192,
/// })?;
/// ```
pub fn create(config: VmConfig) -> Result<Vm> {
    // ...
}
```
