# Core Development Principles

### 1. Rule Zero: Definition of Done is Non-Negotiable
All work on features, bug fixes, or refactors is governed by the "Definition of Done" checklist below. No task is considered complete until every item is satisfied. Partial implementations are unacceptable.

### 2. Maintainability First
Code should be written for the human who will read it next. Prioritize clarity, simplicity, and explicitness over clever or overly concise solutions. The goal is to lower cognitive load for future developers.

### 3. Ownership and End-to-End Thinking
You are not just writing a function; you are delivering a capability. Consider the full lifecycle of your change: configuration, runtime behavior, failure modes, testing, documentation, and monitoring.

### 4. No Shortcuts
- Avoid "TODO" comments as implementation substitutes
- Don't disable linting rules without strong justification
- Always address root causes, not symptoms
- Handle errors gracefully rather than using unsafe operations

## Definition of Done Checklist

A feature is complete if and only if all of the following are true:

### ☐ 1. End-to-End Implementation
- Feature is fully integrated into the application runtime
- Connected to configuration systems where applicable
- Produces tangible, functional effects
- Not hidden behind feature flags (unless for operational control like A/B testing)

### ☐ 2. Comprehensive Automated Testing
- **Unit Tests**: Cover specific logic, edge cases, and error conditions for new functions/classes
- **Integration Tests**: Verify feature works correctly with other system components
- **API Tests**: For new endpoints or interfaces
- **Database Tests**: For data persistence changes

### ☐ 3. Documentation Updated
- **User Documentation**: README or relevant docs explain the feature and its configuration
- **API Documentation**: All public functions/methods/classes have clear documentation
- **Configuration Reference**: New configuration options are documented with examples
- **Code Comments**: Complex logic is explained inline

### ☐ 4. Observability Implemented
- **Logging**: Structured logs at appropriate levels (info, warn, error, debug) with context
- **Metrics**: Performance counters, operation counts, latency measurements where relevant
- **Error Tracking**: Errors logged with sufficient context for debugging

### ☐ 5. Quality Gates Pass
- All linting rules pass without warnings
- Code formatting is consistent
- Security scanning passes
- Performance benchmarks (if applicable) are within acceptable ranges
- Build pipeline completes successfully

## Code Quality Standards

### Complexity Management
- **Extract Functions**: Break down complex functions into smaller, well-named helper functions
- **Use Appropriate Data Structures**: Replace complex conditional logic with data-driven approaches
- **Apply Design Patterns**: Use established patterns (Builder, Strategy, etc.) to simplify complex logic
- **Leverage Language Features**: Use idiomatic language constructs (iterators, functional programming, etc.)

### Code Organization
- **Clear Public APIs**: Design modules with minimal, well-defined public interfaces
- **Separation of Concerns**: Keep business logic separate from infrastructure code
- **Consistent Naming**: Use clear, descriptive names for variables, functions, and classes
- **Immutability Preference**: Default to immutable data structures unless mutation is required

## Error Handling Guidelines

### Language-Specific Best Practices
- **Typed Languages**: Use proper error types (Result types, checked exceptions, etc.)
- **Dynamic Languages**: Implement consistent error handling patterns
- **Always Handle Errors**: Never ignore or suppress errors without explicit justification
- **Provide Context**: Include sufficient information for debugging when errors occur
- **Fail Fast**: Detect and report errors as early as possible

### Error Recovery
- Implement graceful degradation where appropriate
- Use circuit breaker patterns for external dependencies
- Log errors with correlation IDs for distributed systems
- Provide meaningful error messages to users

## Dependency Management

### Adding Dependencies
- **Minimize Dependencies**: Implement functionality internally if reasonably possible
- **Vet New Dependencies**: Check maintenance status, security history, and community support
- **Security Scanning**: Ensure no known vulnerabilities in dependencies
- **Version Pinning**: Use specific versions for reproducible builds
- **License Compatibility**: Verify all dependencies are license-compatible

### Dependency Updates
- Regular security updates for dependencies
- Test thoroughly after dependency updates
- Document breaking changes in upgrade guides

## Testing Requirements

### Test Coverage
- **Unit Tests**: Cover all public functions and critical private functions
- **Integration Tests**: Test component interactions and data flow
- **End-to-End Tests**: Verify complete user workflows
- **Performance Tests**: For performance-critical features
- **Security Tests**: For security-sensitive components

### Test Quality
- Tests should be deterministic and isolated
- Use descriptive test names that explain the scenario
- Mock external dependencies appropriately
- Test both happy path and error conditions
- Maintain test data and fixtures properly

## Documentation Standards

### Code Documentation
- Document public APIs with parameters, return values, and usage examples
- Explain complex algorithms and business logic
- Include architectural decision records (ADRs) for significant decisions
- Keep documentation up-to-date with code changes

### User Documentation
- Installation and setup instructions
- Configuration options and examples
- Usage examples and tutorials
- Troubleshooting guides
- Migration guides for breaking changes

## Build and Deployment

### Pre-commit Requirements
All code changes must pass these checks before merging:
1. Code formatting (language-specific formatter)
2. Linting with zero warnings
3. All tests passing
4. Security scanning
5. Build verification
6. Documentation generation (if applicable)

### Continuous Integration
- Automated testing on multiple environments
- Security scanning in CI pipeline
- Performance regression testing
- Deployment verification tests
- Rollback procedures documented and tested

## Security Guidelines

### Secure Coding Practices
- Input validation and sanitization
- Proper authentication and authorization
- Secure handling of sensitive data
- Protection against common vulnerabilities (OWASP Top 10)
- Regular security reviews and updates

### Data Protection
- Encrypt sensitive data in transit and at rest
- Implement proper access controls
- Log security events appropriately
- Follow data retention policies
- Implement secure backup and recovery procedures