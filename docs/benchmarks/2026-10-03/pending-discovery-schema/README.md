# Control-plane discovery schema enforcement

The administrator discovery route now validates the bounded node JSON response before returning it publicly. It accepts exactly registrations/nextCursor, at most 32 rows with exactly sandboxID/kind, valid IDs, named/unnamed kinds, strictly ascending unique IDs beyond the requested cursor, and either a null cursor or a full-page cursor equal to its final ID. Responses with unknown fields, including envdAccessToken, are refused with a generic 502 error.

All 89 cluster library tests pass, with one pre-existing ignored test. The new malformed-page corpus rejects capability fields at both levels, unsupported kind, invalid ID, short-page cursor, missing cursor field, duplicate IDs and oversized pages. Additional assertions reject a row at the requested cursor and a wrong continuation cursor. Valid empty/single/full pages pass. This tests the exact validation helper used by the route; malformed remote HTTP response and updated KVM runtime verification remain pending.

Production node/CLI formats are unchanged. The updated 133-file permitted catalog matches root and isolated checkout. Snapshots, patch and test log preserve provenance. All compilation ran in the isolated checkout; protected root core files remain excluded. No performance or competitor advantage is claimed.
