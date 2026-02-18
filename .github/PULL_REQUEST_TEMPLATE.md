## Description

Brief description of the changes.

## Type of Change

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to change)
- [ ] Documentation update
- [ ] Refactoring (no functional changes)
- [ ] CI/CD or infrastructure change

## Related Issues

Closes #

## Checklist

- [ ] Code compiles without errors (`cargo check --workspace --exclude hv1-core --exclude hv1-boot`)
- [ ] All tests pass (`cargo test --workspace --exclude hv1-core --exclude hv1-boot`)
- [ ] No new clippy warnings (`cargo clippy --workspace --exclude hv1-core --exclude hv1-boot -- -D warnings`)
- [ ] Code is formatted (`cargo fmt --all -- --check`)
- [ ] Documentation updated if needed
- [ ] CHANGELOG.md updated for user-facing changes

## Testing

Describe how you tested these changes.
