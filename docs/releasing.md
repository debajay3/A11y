# Publishing A11y updates

A11y 0.2.0 introduces signed desktop updates. Previously distributed builds have updates disabled; users must install this version manually once. After that, versioned builds check GitHub five seconds after launch and once a day. Users choose **Update now**, then **Restart to update**. Existing save/leave-session prompts run before installation. Preferences can disable automatic checking.

## Publish a desktop release

1. Merge and test the change on `main`. Keep the collaboration wire protocol compatible with the deployed server.
2. Choose a new semantic version, for example `0.2.1`.
3. Run `git tag -a v0.2.1 -m 'A11y 0.2.1'` and `git push origin v0.2.1`.
4. Watch the **Release** GitHub Actions workflow. It builds Windows x64 (installer and portable ZIP), macOS universal (DMG and update tarball), and Linux x64.
5. All platform builds must succeed before the workflow signs and publishes the combined manifest. Check the release assets and install the update on a test machine before notifying users.

Source pushes alone do not publish updates. Pre-release tags such as `v0.2.1-rc.1` are offered only to clients already on a pre-release. A manual workflow run packages artifacts without publishing or enabling updates. `A11Y_VERSION` enables the updater only in explicitly stamped builds. Release builds ignore the development `A11Y_UPDATE_URL` override.

## Signing and trust

The repository Actions secret `A11Y_UPDATE_SIGNING_KEY` contains a base64 Ed25519 private key. Only its public key is committed at `crates/a11y-app/assets/update-public-key.txt`. Never put the private key, a GitHub token, or a workspace key in app packages or source control. Keep a secure administrator backup of the private key; replacing the pinned public key requires a deliberate key migration or manual reinstall.

`scripts/sign-update.py` signs the exact bytes of `SHA256SUMS` and writes `SHA256SUMS.sig`. The signed manifest includes the release version and the collaboration protocol derived from source. The app verifies the signature, version, protocol, and asset SHA-256 before unpacking anything. Downloads are restricted to this repository's HTTPS release URLs and GitHub's asset CDN. No GitHub credentials are shipped to users.

The signature authenticates A11y releases; it does not replace Apple Developer ID notarization or Windows Authenticode signing. Those certificates still need to be supplied for trusted first-install distribution without OS warnings. macOS packages are ad-hoc signed; Windows packages use a static C runtime.

## Recovery

A failed download or signature check leaves the installed app untouched. The existing installer rolls back failed file swaps. On macOS the last replaced bundle is retained as `<app>.previous`; quit the app before restoring it. This is a manual recovery copy, not automatic crash detection. Windows installer/portable users can reinstall a previous release. Shared server data and local libraries are stored separately and must not be deleted during reinstall. Back up documents before any release that changes their format: restoring an executable does not undo a data migration.

## Collaboration server updates

Desktop updates never replace or restart the shared-library server. For the current deployment, your Mac remains the server and must stay online. AWS deployment can later take over that role.

For a server change:

1. Test the release against the current desktop and new desktop, using a copy of the workspace.
2. Schedule a maintenance window and ask everyone to save and disconnect.
3. Stop the server gracefully. Back up the **entire workspace**, including catalog, documents, workspace key and per-document identity keys, to a private location.
4. Keep the old server executable. Build/install the tested new server executable and start it with the same workspace directory, address and port.
5. Verify authenticated file listing, opening an existing shared file, editing from two machines, and persistence after a server restart.
6. If verification fails, stop the new server and restore the old executable and matching workspace backup before reopening access.

Do not include the live workspace in desktop release archives. A protocol-changing desktop release is rejected by older updaters and needs a coordinated server/client migration. Matching protocol numbers alone are not a substitute for compatibility tests.
