# Security Guidelines

> **Baseline — adapt to the project.** The list below leans toward web apps. When
> scaffolded into a project, drop what doesn't apply and add what does:
> - **CLI / library:** focus on input parsing, path traversal, supply-chain (deps).
> - **Proxy / network service:** SSRF, request forwarding, secrets not logged.
> - **Data pipeline:** PII handling, injection at every parse boundary.

## Always applies

- [ ] No hardcoded secrets (API keys, passwords, tokens) in source.
- [ ] All external input validated at the boundary.
- [ ] Parameterized queries — never string-format SQL with input.
- [ ] Error messages and logs don't leak secrets or sensitive data.

## Web-app specific (drop for CLI/library)

- [ ] XSS prevention (escape/sanitize rendered output)
- [ ] CSRF protection on state-changing endpoints
- [ ] Authentication / authorization verified on every protected route
- [ ] Rate limiting on public endpoints

## Secret Management

- NEVER hardcode secrets in source code.
- ALWAYS use environment variables or a secret manager.
- Validate that required secrets are present at startup.
- Rotate any secret that may have been exposed.

## Security Response Protocol

If a security issue is found:
1. STOP immediately.
2. Fix CRITICAL issues before continuing.
3. Rotate any exposed secrets.
4. Review the codebase for similar instances.
