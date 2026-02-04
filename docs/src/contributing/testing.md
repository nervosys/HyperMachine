# Testing

Guidelines for writing and running tests in HyperMachine.

## Test Structure

```
crates/
  hv2-core/
    src/
    tests/           # Integration tests
      integration.rs
    benches/         # Benchmarks
      crypto_bench.rs
```

## Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vm_creation() {
        let config = VmConfig {
            name: "test".into(),
            cpu_cores: 2,
            memory_mb: 4096,
        };
        
        let vm = Vm::create(config).unwrap();
        assert_eq!(vm.name(), "test");
        assert_eq!(vm.status(), VmStatus::Created);
    }
    
    #[test]
    fn test_vm_not_found() {
        let result = Vm::get("nonexistent");
        assert!(matches!(result, Err(VmError::NotFound(_))));
    }
}
```

## Integration Tests

```rust
// tests/integration.rs
use hypermachine::prelude::*;

#[test]
fn test_full_vm_lifecycle() {
    let hm = HyperMachine::new_local().unwrap();
    
    // Create
    let vm = hm.create_vm("integration-test", 2, 4096).unwrap();
    assert_eq!(vm.status(), VmStatus::Created);
    
    // Start
    vm.start().unwrap();
    assert_eq!(vm.status(), VmStatus::Running);
    
    // Execute
    let result = vm.exec("echo hello").unwrap();
    assert_eq!(result.stdout.trim(), "hello");
    
    // Stop
    vm.stop().unwrap();
    assert_eq!(vm.status(), VmStatus::Stopped);
    
    // Delete
    vm.delete().unwrap();
}
```

## Running Tests

```bash
# All tests
cargo test --workspace

# Specific crate
cargo test -p hv2-core

# Specific test
cargo test test_vm_creation

# With output
cargo test -- --nocapture

# Ignored tests (require hardware)
cargo test -- --ignored
```

## Benchmarks

```bash
cargo bench -p hv2-core --bench crypto_bench
```
