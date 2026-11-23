# A11y accounts (local deployment)

The library server owns authentication. No Keycloak, Java, OAuth provider, shared workspace key, or public signup is used by this release.

## Start and bootstrap

Build `a11y-library-server`. In a private workspace directory, run:

```
a11y-library-server --bootstrap-admin WORKSPACE_DIR USERNAME
```

Supply a temporary password on stdin, never as a command-line argument. The command accepts at most 256 bytes and refuses setup if any account already exists. Use at least 14 characters. The initial administrator must change the password before accessing files or administration.

Place `server-cert.pem` and `server-key.pem` in the workspace directory. Then start:

```
a11y-library-server WORKSPACE_DIR 0.0.0.0:49180
```

Network listeners require TLS. Plain HTTP is supported only on loopback for server testing; the desktop requires HTTPS. This app build trusts the public local-workspace CA at `crates/a11y-app/assets/server/local-ca.pem`, in addition to public CAs. The private CA key and server key must NEVER be committed or distributed with clients. The current local certificate covers 127.0.0.1, localhost and 10.20.25.165; assign the Mac a stable LAN IP. Changing the address requires a matching certificate and updated server address, not disabling TLS verification. The server certificate expires in one year; renew it using the same private CA. An AWS hostname should use a publicly trusted certificate.

Account data is in `accounts.sqlite` (SQLite WAL, FULL synchronous writes). Back up the database with SQLite's backup API or stop the server before copying the database and its WAL. This is a single-process local deployment; PostgreSQL migration remains future work. Keep the workspace directory private and back it up together with the document catalog and files. Bootstrap credentials are never in release artifacts.

## User experience

Users sign in in A11y with the HTTPS server address, username and temporary password, then choose their own password. The initial platform administrator owns this single workspace and its existing content. In the username menu select **Admin · People and access** (also available in Shared library). Use Refresh users after account changes. Administrators can create accounts, enable/disable them, promote/demote administrators, reset passwords, and assign project/file rules. At least one active administrator must remain.

Default library access is none/viewer/editor. File rules override project rules, which override the default. None is an explicit block. Administrators have full access to this initial single workspace; the UI says so explicitly. These are not the future multi-organization inherited ACLs. Project names currently identify projects; file IDs identify files. Teams, nested shared folders, Commenter and Manager are not implemented in this slice. Those require the metadata migration described in the architecture document.

Disabling accounts, resetting passwords, changing account roles and changing grants revoke that user's sessions. The API filters file catalogs and checks publish/open operations. The collaboration transport validates current sessions before admitting a guest and the host checks authorization during collaboration. Resume secrets are bound to the authenticating session, preventing one account resuming another account's document identity. Old workspace keys no longer work. Credentials travel over HTTPS; document collaboration remains the existing encrypted QUIC protocol.

Remembered sessions are stored in the OS credential vault and expire after seven days. Temporary-password sessions expire after ten minutes and cannot open files or administer accounts. Local libraries are partitioned by server and immutable user ID. Existing pre-account local libraries remain on disk; users can open/import their previous local files. This release does not encrypt document caches or provide a durable offline operation queue. Previously signed-in users can continue local editing within the cached session lifetime, but crash-safe offline shared-file replay remains future work. Revocation cannot immediately reach a disconnected device.

## Acceptance checks

- First login cannot list files or call administrator endpoints until password changed.
- Anonymous, expired, disabled and legacy-key requests are denied.
- Member cannot create/reset/disable accounts or grant permissions.
- Viewer sees permitted files but cannot publish or edit through collaboration.
- Project/file restrictions filter listing and prevent joining directly.
- Reset, signout and disable invalidate sessions; live connections are cut.
- Resume token from another session is denied.
- Last administrator cannot be disabled/demoted.
- Restart preserves accounts and grants, but never stores plaintext passwords or bearer tokens in SQLite.
- Desktop login and forced-password screens render; malformed/non-HTTPS server addresses are rejected.
- Run existing editor and collaboration regression tests before publishing signed desktop updates.
