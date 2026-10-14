# Android uniform preparation budget, 2026-10-14

This follows client `41210c308`. At the user's request, Android now grants every
native-P2P preparation attempt up to five seconds, including the first connection,
an app restart and an unknown or changed server. The previous server-support cache
has been removed; successful authentication is still required before UDP probing.

Temporary registration and the Begin RPC share the same five-second window. The
45-second overall attempt deadline still starts before registration. An explicit
unsupported response or an authentication failure ends preparation immediately;
a silent old server may now delay original-flow fallback by up to five seconds
instead of one. Cancellation remains effective while preparation is pending.

## Regression surface

The only changed runtime file is `src/native_p2p/android_registration.rs`: a fixed
five-second preparation deadline replaces the cached one/five-second choice.
Its existing timeout and session-deadline clamp are preserved. The already-pinned
common API accepts this timeout without another submodule change. Windows/Linux,
the server, protocol, UI, JNI, service lifecycle, authentication and permissions
have no changes. No new dependency, option, retry or background task is introduced.

## Validation and package

- Host native-P2P regressions: **8 passed, 0 failed**. Existing cases exercise a first-contact
  signed reply delayed by 1,200 ms, reject an invalid signature promptly, and use
  1,500/5,500 ms registration delays to check the shared five-second preparation
  deadline and cleanup. The authenticated TCP fixture simulates an active sharing
  registration; these are loopback protocol tests, not a remote-control session.
- Android arm64 release Rust library and debug APK: built successfully. The
  Android dependency tree is unchanged from the preceding package.
- APK v1/v2 signature verification passed; arm64-v8a is the only packaged ABI.
  The compiled and packaged Rust libraries have identical SHA-256 hashes.

Logs and the updated APK are under
`artifacts/android-uniform-preparation-20261014/` at the workspace root.
The package is `rustdesk-native-p2p-android-arm64-uniform-preparation.apk`.

| Artifact | SHA-256 |
|---|---|
| APK | `6E9295AA6B4879D05EED2E2923518043427843C34C9A624AB3978FFC94826678` |
| Compiled and packaged Rust library | `E654D86C90CB30720192A7CD1519C5BE08DC367C56195342D0CA16A0CDFB7E68` |

The host test command is:

```text
cargo test --locked --offline --release --lib --features flutter,hwcodec native_p2p -- --test-threads=1
```

`ANDROID.md` describes the build environment; `build-rust.sh`, build/test logs and
`validation.json` are retained beside the APK. The preceding APK remains in
`artifacts/android-preparation-budget-20261014/`.

Android real-device acceptance remains pending. Host protocol tests and a package
build do not establish phone connectivity or background behavior. This work does
not install the APK or change the running services, TUN, routes or firewall.

## Rollback

Disable the existing native-P2P switch to use the original connection strategy,
or reinstall the preceding APK to restore the adaptive preparation budget.
