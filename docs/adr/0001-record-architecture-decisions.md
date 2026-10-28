# 1. Record architecture decisions

Date: 2026-10-28

## Status

Accepted

## Context

Praxis is a self-improving agent runtime that evolves agent strategies through an
improvement loop. As the codebase grows, architectural decisions need to be
recorded for future reference.

## Decision

We will use Architecture Decision Records (ADRs), as described by Michael Nygard,
stored in `docs/adr/` with sequential numbering (0001, 0002, ...).

## Consequences

- ADRs are version-controlled alongside the code
- Each ADR captures context, decision, and consequences
- New ADRs are added when significant architectural choices are made
