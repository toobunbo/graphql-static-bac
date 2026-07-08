# Selected Runtime Routes - Deck

Date: June 9, 2026

This subset contains all 11 `open` routes and 10 representative `unknown`
routes selected from the full 867-route S3 artifact.

Machine-readable artifacts:

```text
output/routes.deck.json
output/deck.unknown_selection.json
```

No runtime behavior is assumed here. The schema establishes that:

- `Deck` implements `Node`;
- `Deck.visible` is a non-null Boolean;
- `createDeckInput.visible` and `editDeckInput.visible` are required;
- decks can be selected directly and reached through cards and user profiles.

The runtime question is whether `visible=false` restricts another requester
from discovering or reading the deck through each route.

## Open - test all 11

| Case | Selector | Witness |
|---|---|---|
| DECK-O01 | `Query.node(id)` | `Query.node -> ... on Deck` |
| DECK-O02 | `Query.nodes(ids)` | `Query.nodes -> ... on Deck` |
| DECK-O03 | `Query.deck(slug)` | `Query.deck -> Deck` |
| DECK-O04 | `BaseballUserSportProfile.deck(name)` | `node(BaseballUserSportProfile) -> deck -> Deck` |
| DECK-O05 | `BaseballUserSportProfile.deck(slug)` | `node(BaseballUserSportProfile) -> deck -> Deck` |
| DECK-O06 | `FootballUserSportProfile.deck(name)` | `node(FootballUserSportProfile) -> deck -> Deck` |
| DECK-O07 | `FootballUserSportProfile.deck(slug)` | `node(FootballUserSportProfile) -> deck -> Deck` |
| DECK-O08 | `NBAUserSportProfile.deck(name)` | `node(NBAUserSportProfile) -> deck -> Deck` |
| DECK-O09 | `NBAUserSportProfile.deck(slug)` | `node(NBAUserSportProfile) -> deck -> Deck` |
| DECK-O10 | `UserSportProfileInterface.deck(name)` | `user -> anyUserSportProfile -> deck -> Deck` |
| DECK-O11 | `UserSportProfileInterface.deck(slug)` | `user -> anyUserSportProfile -> deck -> Deck` |

For each selector, compare:

- the deck owner;
- an authenticated non-owner;
- an anonymous requester where supported;
- equivalent `visible=true` and `visible=false` decks.

At minimum, project `id`, `slug`, `name`, `visible`, `user`, and a small card
sample. Distinguish between complete denial, object redaction, and card
collection filtering.

## Unknown - selected 10

| Case | Behavior covered | Witness |
|---|---|---|
| DECK-U01 | card-selected interface relation | `anyCard(assetId) -> AnyCardInterface.decks -> Deck` |
| DECK-U02 | card-selected concrete relation | `anyCard(slug) -> ... on Card -> Card.decks -> Deck` |
| DECK-U03 | user-selected football collection | `user(slug) -> footballUserProfile -> decks.nodes -> Deck` |
| DECK-U04 | user-selected baseball collection | `user(slug) -> baseballUserProfile -> decks.nodes -> Deck` |
| DECK-U05 | user-selected NBA collection | `user(slug) -> nbaUserProfile -> decks.nodes -> Deck` |
| DECK-U06 | highlighted deck through profile interface | `user(slug) -> anyUserSportProfile -> highlightedDeck -> Deck` |
| DECK-U07 | user ID to interface collection | `userById(id) -> anyUserSportProfile -> decks.nodes -> Deck` |
| DECK-U08 | global-ID user to interface collection | `node(User) -> anyUserSportProfile -> decks.nodes -> Deck` |
| DECK-U09 | known deck to related decks through a card | `deck(slug) -> cards.nodes -> decks -> Deck` |
| DECK-U10 | known deck to owner's other decks | `deck(slug) -> user -> anyUserSportProfile -> decks.nodes -> Deck` |

The exact `route_id` and verification question for each unknown case are in
`output/deck.unknown_selection.json`.

## Main questions

1. Do direct identity selectors enforce `Deck.visible`?
2. Do `deck(name|slug)` resolvers behave consistently across all sports?
3. Are invisible decks removed from another user's `decks` collection?
4. Can `highlightedDeck` expose a deck after it becomes invisible?
5. Does `Card.decks` reveal invisible decks containing a known card?
6. Can one known deck pivot through its owner or cards to other invisible decks?
7. Do `user`, `userById`, and `node(User)` apply equivalent visibility rules?

## Request batching

The selected cases can be covered with fewer operations:

- DECK-O01 and DECK-O02 can share known public/private global IDs.
- DECK-O04 through DECK-O09 can be grouped by sport profile.
- DECK-O10, DECK-O11, and DECK-U06 can share one user-profile operation.
- DECK-U01 and DECK-U02 can share one selected card where its concrete type is `Card`.
- DECK-U03 through DECK-U05 can share one user query.
- DECK-U07 and DECK-U08 can use the same target user through different selectors.
- DECK-U09 and DECK-U10 can start from the same known deck.

Keep one result object per `case_id` even when cases share a GraphQL request.
This preserves the join back to the S3 route.

## Result fields

Record one JSON object per case:

```json
{
  "case_id": "DECK-U01",
  "route_id": "route:sha256:...",
  "requester": "owner|authenticated_non_owner|anonymous",
  "seed_visibility": true,
  "runtime_feasibility": "confirmed|contradicted",
  "target_reached": true,
  "returned_decks": [
    {
      "id": "Deck:...",
      "slug": "...",
      "visible": true,
      "owner_id": "User:..."
    }
  ],
  "selector_controls_sink": "yes|no|unknown",
  "visibility_enforced": "yes|no|unknown",
  "authorization_result": "allowed|denied|filtered|not_tested",
  "graphql_errors": [],
  "notes": ""
}
```

For a private seed, use `"seed_visibility": false`. A route is especially
important when a non-owner reaches that same private deck identity rather than
merely another public deck sharing a card or owner.
