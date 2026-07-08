# Research Prompt: AnnouncementNotification Cross-Account Behavior

Analyze the security implications of this authorized GraphQL test:

```text
Token: Account A
Input: AnnouncementNotification ID belonging to Account B

Query.node(notificationId)
  -> AnnouncementNotification.user
  -> CurrentUser
```

Observed behavior:

- Account A can resolve Account B's `AnnouncementNotification` by global ID.
- `AnnouncementNotification.user` resolves to Account B, not the authenticated Account A.
- Continuing through `user.myWatchlists` attempts to return Account B's five
  Watchlists.
- Public Watchlists may resolve, while private Watchlists produce
  `Not authorized to access Watchlist`.

Please assess:

1. Whether cross-account access to the notification object or its metadata is
   itself an authorization/privacy issue.
2. Which notification fields could expose sensitive information before a
   downstream authorization check occurs.
3. Whether related paths from `AnnouncementNotification.user` expose other
   Account B data without equivalent guards.
4. What controlled two-account tests would establish impact without modifying
   data.
5. Whether this should be reported as intended global-ID behavior, an
   information leak, or a broader object-level authorization weakness.

Do not assume that the GraphQL type name `CurrentUser` means the authenticated
principal; runtime testing proved that it can represent the notification owner.
