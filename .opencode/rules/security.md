# Security (NeoDOS Kernel)

## Kernel Security Model

- **Ring 0**: Kernel — trusted, required for NEM drivers
- **Ring 3**: User mode — all .NXE binaries
- **Ob security**: SID/Token/ACL on every named object
- **SeAccessCheck**: Gates every ObOpen
- **Capabilities**: NEM drivers declare caps at load time

## Mandatory Pre-Commit Security Checks

- [ ] No unsafe block without // Safety: comment
- [ ] No kernel addresses leaked to user mode
- [ ] All ObOpen paths check SeAccessCheck
- [ ] No unwrap()/expect() on user-supplied data
- [ ] Buffer sizes validated before use
- [ ] No raw pointer arithmetic without bounds

## Ob Security

Every named object has:
- Owner SID
- Group SID
- DACL (discretionary access control list)
- SACL (system audit control list)

Access check flow:
```
ObOpen(name, desired_access)
  → lookup object
  → SeAccessCheck(sd, token, desired_access)
  → if denied → STATUS_ACCESS_DENIED
  → if granted → return ObHandle
```

## Secret Management

- Kernel configs via registry (Cm), not hardcoded
- No secrets in kernel debug output
- Boot-time secrets wiped after init phase

## Response Protocol

If security issue found:
1. STOP immediately
2. Use security-reviewer agent
3. Fix CRITICAL issues before continuing
4. Review entire codebase for similar issues
