---
paths:
  - "**/*.go"
  - "**/*.rs"
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.py"
  - "**/*.java"
  - "**/*.swift"
  - "**/*.cpp"
  - "**/*.cc"
  - "**/*.hpp"
  - "**/*.h"
---
# Always Write Tests Alongside Code

When creating or modifying source code, you MUST write or update the corresponding tests in the SAME operation. Never leave test creation as a follow-up task.

## Per-Language Test Location

| Language | Test Location | Example |
|----------|--------------|---------|
| Go | `_test.go` file next to source | `user.go` → `user_test.go` |
| Rust | `#[cfg(test)] mod tests` at bottom of same file | inline in `user.rs` |
| TypeScript/JS | `.test.ts` or `.spec.ts` next to source | `user.ts` → `user.test.ts` |
| Python | `test_*.py` next to source or in `tests/` | `user.py` → `test_user.py` |
| Java | Mirror package in `src/test/java/` | `User.java` → `UserTest.java` |
| Swift | `Tests/` directory with matching structure | `User.swift` → `UserTests.swift` |
| C++ | `tests/` mirroring `src/` (GoogleTest + CTest) | `user.cpp` → `tests/user_test.cpp` |

## Rules

1. **New file** → create the corresponding test file/module at the same time
2. **Modified function** → update or add tests covering the change
3. **Bug fix** → write a regression test that reproduces the bug BEFORE fixing it
4. **Deleted code** → remove the corresponding tests (no orphaned tests)

## What to Test

- Public functions and methods (always)
- Error paths and edge cases (always)
- Private helpers only if they contain non-trivial logic

## What NOT to Do

- Do not defer tests to a later step — write them NOW
- Do not write tests in a separate commit unless explicitly asked
- Do not skip tests because "it's a small change"
