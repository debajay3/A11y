# A11y accounts, permissions and offline sync

Status: target architecture and staged implementation. The current shared-key demo is NOT an authenticated multi-user deployment. Do not advertise login enforcement or per-file permissions until the server and collaboration acceptance tests below pass.

## Decisions

- Keep the Rust/egui desktop editor and the existing document/collaboration engine.
- A11y owns authentication and account administration; Keycloak is no longer part of the target architecture. Login and the Admin panel are inside the desktop app. Public registration is disabled.
- The designated first administrator is provisioned using a one-time local server setup command, never a shipped username/password or public bootstrap endpoint. Subsequent administrators are assigned by authorized administrators. Keep at least one active platform administrator; require recent authentication for privileged account changes.
- Store only salted Argon2id password hashes on the server, using a maintained implementation and versioned parameters. Start at no less than the OWASP minimum (19 MiB, two iterations, one lane), benchmark the server, and bound concurrent hashing work. Neither plaintext passwords nor password hashes belong in desktop settings, logs, shared catalogs or sync payloads.
- Use cryptographically random opaque sessions, storing only token digests server-side. Enforce expiry, revocation, rotation and account status in the API. Store remembered credentials in macOS Keychain / Windows Credential Manager. A restricted first-login session permits password change and signout only; it must not unlock files or administration.
- Apply generic login errors, account/source throttling, bounded request sizes and an audit trail without secrets. Password resets issue short-lived, single-use temporary credentials and revoke existing sessions. No email dependency is required for local admin-issued credentials; do not expose an unauthenticated password-reset shortcut.
- Build a Rust Axum/Tokio API and authenticated WebSocket collaboration gateway. Start with one service and one authoritative writer per active document, not a fleet of microservices. Reuse the existing sync engine behind the new transport.
- PostgreSQL holds users, password hashes, sessions, organizations, memberships, projects, folder hierarchy, grants, document revisions, accepted operation IDs and audit records. Private object storage holds document snapshots, thumbnails and assets. Locally use PostgreSQL plus a filesystem storage adapter; on AWS use RDS PostgreSQL and private S3.
- Desktop caches authorized documents and pending changes in a transactional local SQLite database, partitioned by account, organization and file. Encryption keys belong in the OS credential store. Persist an operation before showing it as locally saved. Keep local-save and server-sync indicators separate.

## Deployment shape

Desktop (Mac/Windows) -> HTTPS API / authenticated WSS -> PostgreSQL + private object storage
Desktop login -> A11y authentication API -> account and session database
Designated administrator -> A11y Admin panel -> server-authorized administration API

Local proof: bind the authentication API to 127.0.0.1, use a separate test workspace, and keep the working shared-key demo unchanged. Loopback HTTP is development-only. Testing from other machines requires a stable LAN hostname and trusted HTTPS certificate; a localhost URL on another Mac points to that Mac, not this server.

AWS: ALB with HTTPS/WSS, ECS or EC2 containers for A11y API/authentication/collaboration, RDS PostgreSQL in private subnets, private S3, Secrets Manager, CloudWatch logs/alarms, backups and point-in-time recovery. Start with one collaboration writer; before horizontal scaling add a durable room-ownership lease with fencing. Redis/pubsub is optional coordination, never the source of truth. No per-document public UDP ports in the authenticated target deployment.

## Account lifecycle

1. Administrator creates an account in the admin panel, assigns organization and teams, and issues a temporary password or activation link.
2. User clicks Sign in in A11y. The app presents its own username/password form and sends credentials to the A11y server over TLS (loopback HTTP only in development). No public Sign up button appears.
3. First login requires a new password. Identify accounts by immutable server-generated user IDs, never by email or display name. MFA is a future explicit feature, not supplied by this change.
4. API checks account active status, membership and current policy, then returns authorized catalog and per-file capabilities.
5. Sign out revokes the online session and clears local credentials/leases. Retain encrypted unsynced drafts under their originating account, inaccessible in another account's session. Do not delete work silently.
6. Disable/reset/remove actions invalidate API sessions and disconnect live rooms. Revoke sessions transactionally with account changes and notify active connections; validate current session/account status on every authorized request and document operation.

## Permissions

Hierarchy: organization -> project -> folder (nested) -> file. Teams are groups within an organization, not a required parent of projects. Grant access to teams by default, with individual exceptions when needed.

Organization roles: owner (organization lifecycle and administrator management), administrator (organization memberships, teams, projects and grants), member. Platform identity administration is separate: an organization administrator may disable membership in their organization, but cannot disable a global account or reset another organization's user credentials. The initial A11y platform administrator manages global accounts. Organization-scoped administrators manage their own memberships and grants. The server decides the scope; hiding the Admin tab alone is not authorization. Administrative control does not silently grant access to design contents: privileged access must be explicitly granted and audited.

| Resource role | Read | Comment | Edit contents | Move/delete | Manage sharing |
|---|---|---|---|---|---|
| Viewer | Yes | No | No | No | No |
| Commenter | Yes | Yes | No | No | No |
| Editor | Yes | Yes | Yes | No | No |
| Manager | Yes | Yes | Yes | Yes | Yes |

Export/download is an additional capability, not an implication of editing. A recipient who has already seen or downloaded data cannot be made to forget it by revocation.

Rules:
- Deny access unless an active user and active organization membership have an applicable grant.
- Inherit grants from project/folder; a restricted folder or file starts a new inheritance boundary. A direct Viewer grant does not reduce an inherited Editor grant: restrict inheritance first, then grant Viewer. Show effective permissions and their source in the UI.
- Combine applicable team/direct grants at the highest allowed role within the boundary. Avoid explicit-deny rules in the first release.
- Files/folders cannot reference a parent in another organization; enforce composite tenant foreign keys and cycle checks. Moving a resource requires permission on both source and destination and recomputes inherited access.
- List/search/thumbnail/export/history/collaboration/AI/MCP routes all check permissions. Unauthorized names must not leak via catalogs or search. Share links identify a file but do not authorize access.
- The server chooses the collaboration role from current policy. Never trust a client-supplied role, user ID, organization ID, offline lease or old shared workspace key.

## Offline editing and reconnection

First-ever login requires the server. Previously signed-in users may work on previously authorized cached files while offline. Proposed default: an offline authorization lease lasting at most seven days, configurable by organization; review this duration before rollout. Expiry locks editing but preserves drafts. The lease is signed by the API, bound to account/device/organization/file and a maximum capability; it is not a token accepted for online writes.

Store a durable queue of operation batches with operation UUID, account, device, file, base server revision, payload hash and local sequence. The server must allocate durable per-file/device node-ID ranges; the desktop must restore its allocation counter on restart so two offline devices cannot create colliding node IDs. Cache a base snapshot plus the optimistic local document, retain node-ID allocation identity across restarts, and store assets referenced by queued operations. The existing GuestSession is in-memory and does not yet supply crash-safe offline recovery.

Reconnection:
1. Refresh/revalidate identity and obtain current account/membership/file permissions.
2. Fetch missing server changes from the last acknowledged revision.
3. Rebase queued changes with the collaboration engine; do not upload a full stale snapshot over newer remote work.
4. Commit each accepted batch, document revision and idempotency receipt in one server transaction. Duplicate `(account, device, file, operation_id)` submissions return the original receipt only if payload hashes match.
5. Persist the server receipt before clearing the local queue. Network failure or app restart retries the same IDs.
6. If access is revoked, file deleted, version unsupported, history compacted beyond the base, or a semantic conflict cannot be resolved safely, pause sync and preserve a recovery draft. Do not create a server copy that bypasses the revoked ACL. Copy/export recovery is governed by the organization's data policy.

Automatic sync is the normal path, not a guarantee that every conflict can be merged silently. The UI distinguishes Saved locally, Offline — changes pending, Syncing, Synced, and Needs attention.

Offline trade-off: disabling an account cannot instantly stop an already offline device or retract downloaded data. The lease limits normal-client offline use; the server rejects writes immediately after it knows of revocation. Modifying a client binary can bypass client UI checks, so sensitive data protection must be server-enforced.

## Migration and release gates

1. Replace the isolated Keycloak/OIDC proof with A11y-owned account storage, password hashing, session lifecycle, one-time administrator setup and server-side admin checks. Build login, forced password change and Admin screens inside A11y. Do not change the current user's live credentials or files. Retire the old proof and local Keycloak runtime after replacement verification; they are not dependencies of the installed editor.
2. Authenticated desktop entry screen and token lifecycle; API token validation and account/membership checks. Fail closed on missing auth configuration in managed releases. Development-only bypasses must not be shipped in managed releases.
3. Migrate a COPY of the existing catalog into organizations/projects/folders/files, preserve IDs/document contents, and assign explicit initial grants. Build the A11y permission-admin UI. Back up before production migration.
4. Replace static shared join links/passwords with authenticated, short-lived room admission. Revalidate authorization on operations and live membership changes. Disable old shared-key and peer-to-peer bypass routes for managed files; prevent old clients from reaching the secured workspace.
5. Durable offline queue and restore, rebase, idempotency and recovery UX; reconnect tests on two devices.
6. Management acceptance on the Mac with isolated test data, then AWS staging, then a coordinated client/server production release. Existing demo clients must not be silently locked out by a desktop-only update.

Required tests: anonymous/disabled user denied; public signup denied; ordinary user cannot create/administer accounts; first-login password change; restart/session refresh/signout; expired/revoked/replayed sessions denied; password reset revokes sessions; first-login sessions cannot access files; login throttling; concurrent last-admin deletion/demotion rejected; one-time bootstrap cannot run again; cross-organization access denied; inherited and restricted permissions; viewer cannot edit via API/MCP/WebSocket; revoke an active editor; offline restart then sync; concurrent edits; repeated and out-of-order batches; deleted file while offline; disk-full and server crash; two accounts on one machine never share queues or cached credentials.

## References

- Password storage: https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html
- Session management: https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html
- Authorization enforcement: https://cheatsheetseries.owasp.org/cheatsheets/Authorization_Cheat_Sheet.html


## Current implementation slice

A11y now has native account/password/session storage and in-app sign-in and administration code. The isolated Keycloak/OIDC proof has been removed from the repository. Local accounts use SQLite rather than PostgreSQL while the metadata migration is pending. See `deploy/accounts/README.md` for the exact current behavior and release checks.

The initial platform administrator explicitly owns all content in this single workspace. Current access levels are none/viewer/editor, with file rules overriding project rules and then the workspace default. This is deliberately recorded separately from the future multi-organization role model above. Teams, shared nested folders, Commenter/Manager roles, encrypted caches, durable offline operations and PostgreSQL are not implemented by the initial account release. Existing pre-account local files remain on disk; account libraries are separate.
