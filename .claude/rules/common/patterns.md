# Common Patterns

## Design Patterns

### Repository Pattern

Encapsulate data access behind a consistent interface:
- Define standard operations: findAll, findById, create, update, delete
- Concrete implementations handle storage details (database, API, file, etc.)
- Business logic depends on the abstract interface, not the storage mechanism
- Enables easy swapping of data sources and simplifies testing with mocks

### API Response Format

> **Applies only to APIs you design.** A pass-through proxy or gateway must mirror
> the upstream response format **verbatim** — wrapping it in your own envelope
> breaks client compatibility. Skip this section for proxy/forwarding services.

For services that own their API surface, use a consistent envelope:
- Include a success/status indicator
- Include the data payload (nullable on error)
- Include an error message field (nullable on success)
- Include metadata for paginated responses (total, page, limit)
