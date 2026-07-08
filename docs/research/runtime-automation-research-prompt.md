# Research Prompt: Automating GraphQL Authorization Testing

Research papers, tools, and existing techniques that could help automate
runtime authorization testing for GraphQL APIs.

This is an idea-research task only. Do not produce an implementation plan or
write code.

## Context

Assume we already have:

1. A structural route to a target GraphQL type, including root field, nested
   fields, interface or union fragments, arguments, and argument types.
2. A tool such as InQL that can generate a valid selection set for the target
   type.

An executable test still needs:

```text
structural route
+ GraphQL projection
+ valid argument values/seeds
+ requester identities
+ security oracle
= automated authorization test
```

## Research Questions

### Query generation

- Which tools can generate executable GraphQL queries from introspection?
- Can they handle nested arguments, interfaces, unions, connections, required
  arguments, recursion, and bounded field projections?
- What exactly can InQL generate and what remains missing?

### Seed discovery

- How can a system automatically find valid IDs, slugs, cursors, and other
  argument values?
- Can values be harvested from previous responses, Burp traffic, HAR files,
  logs, or user-provided seeds?
- Which tools infer producer-consumer dependencies between API operations?
- What ideas can be borrowed from stateful REST API fuzzing and property-based
  testing?

### Multiple requester identities

- Which tools replay equivalent requests as owner, authenticated non-owner,
  different roles or tenants, and anonymous users?
- How do they manage sessions, tokens, token refresh, headers, and isolated
  state?

### Security oracles

- How can a tool distinguish an authorization vulnerability from legitimate
  public access?
- Research differential and metamorphic testing for BOLA/IDOR and broken
  object property authorization.
- How should it compare owner and non-owner responses?
- How should it normalize nulls, not-found errors, authorization errors,
  redacted fields, and filtered collections?
- How can it verify that the returned object is the exact private seed rather
  than another public object?
- Which oracles can be generic, and which require domain-specific facts such
  as `visible=false` or `public=false`?

### Stateful execution

- Should tests run independently or as producer-consumer workflows?
- Can response values automatically feed later requests?
- Are state machines, coverage-guided testing, or property-based sequences
  useful here?

## Tools And Research To Survey

Search for relevant work in:

- GraphQL query generation and security testing;
- InQL and other Burp GraphQL extensions;
- stateful REST and API fuzzers;
- producer-consumer dependency inference;
- BOLA/IDOR detection;
- differential authorization testing;
- metamorphic security testing;
- property-based API testing;
- multi-user request replay.

Prefer original papers, official documentation, and source repositories.
Include direct links and note whether each tool is actively maintained.

## Deliverable

Produce a concise research report containing:

1. The most relevant papers and tools.
2. What each one solves and does not solve.
3. Reusable ideas for query generation, seed discovery, requester management,
   and security oracles.
4. Two or three possible high-level integration approaches with tradeoffs.
5. The major unresolved technical questions.
6. A few small experiments that would reduce uncertainty.
7. A recommended next research step.

Clearly distinguish:

- facts verified from primary sources;
- reasonable inferences;
- hypotheses that still require experiments.

Do not assume structurally similar GraphQL routes use the same backend
authorization logic. Do not perform testing against systems without explicit
authorization.
