# Android preparation budget, 2026-10-14

This change follows client `56e716019` / client-common `90015d4`. It relaxes only
the Android native-strategy preparation window for a previously verified server.

## Behavior

| Situation | Combined registration and Begin budget |
|---|---|
| First attempt, app restart or unknown server | 1 second |
| Most recently authenticated server, same endpoint and public key | 5 seconds |
| Previous preparation failed, was cancelled or returned no native session | Back to 1 second |
| Endpoint or configured server key changed | 1 second |

A successful preparation must receive a ticket whose server signature, response
nonce/time, capability, expiry and sealed probe key pass the existing checks.
Neither an ordinary UDP registration response nor possession of a server public
key records native support. No authentication or permission check is skipped.

The client keeps one endpoint/key pair in process memory. The next attempt takes
that record before awaiting registration and restores it only after a valid native
session is returned. This also invalidates stale support when a future is cancelled,
without adding expiry timers, persisted configuration, retries or refresh workers.
An unsupported peer conservatively clears the record as well. Failure later in
NAT punching does not undo a server capability that was successfully verified.

The five seconds cover registration and capability negotiation together; they are
not additional time beyond the original 45-second attempt deadline. The original
fallback remains in place. A server downgraded under the same endpoint/key may
receive one five-second attempt before the client returns to the short deadline.
The first connection on a slow network can still time out at one second: this
change deliberately requires previous authenticated evidence before allowing more.

## Regression surface

- `src/native_p2p/android_registration.rs`: selects the preparation deadline from
  the single authenticated server record. No mediator, JNI, UI, service-lifecycle,
  permission, media or legacy-connection code changes are needed.
- Client-common `native-p2p/src/transport.rs`: adds `Session::begin_with_timeout`
  so Android's longer outer window is not cut short by the old inner RPC timer.
  Existing `Session::begin` callers retain their one-second default. The old
  signature, signed wire messages, server code and strategy algorithms are unchanged.
- The parent pins exactly this client-common API commit. Server/common checkouts
  remain unchanged; no server deployment is needed.

## Validation and package

- Client native-P2P regression tests: **8 passed, 0 failed**. The new loopback
  test obtains an authenticated ticket, then succeeds with a 1,200 ms reply delay;
  a changed key or invalid signature cannot establish the longer budget. It also
  checks the total attempt deadline and permit cleanup. Existing tests cover the
  shared one-second registration/negotiation budget, cancellation, original bypass
  policies and KCP handoff. The fixture simulates an active registration; it is not
  a phone or a cross-network remote-control test.
- Client-common transport tests: **2 passed, 0 failed**, including the original
  one-second old-server fallback. Pinned common commit:
  `cd810d512f4d21dc8268d2c5ca8d8e3067d288fe`.
- Android arm64 release Rust library and debug APK: **built successfully**, using
  the existing locked, offline dependency set. The Android dependency tree is
  unchanged from the preceding review-fix package.
- APK v1/v2 signatures and arm64-only packaging passed verification. The Rust
  library extracted from the APK matches the compiled library byte for byte.

Commands used, with the existing build environment documented in `ANDROID.md`:

```text
cargo test --locked --offline --release --lib --features flutter,hwcodec native_p2p -- --test-threads=1
cargo test --locked --offline --release -p rustdesk-native-p2p transport::tests -- --test-threads=1
cargo ndk --platform 21 --target aarch64-linux-android build --locked --offline --release --lib --features flutter,hwcodec
```

Package and logs are under `artifacts/android-preparation-budget-20261014/` at the
workspace root. The package is
`rustdesk-native-p2p-android-arm64-preparation-budget.apk` (debug-signed).

| Artifact | SHA-256 |
|---|---|
| APK | `E0CA4B905FA6F28A616F305604A3FB9D03B7D64E851B9FC6BCD12CA8569F2937` |
| Compiled and packaged Rust library | `B04D28D3A09025CAAFDA769F37E480E53F2E1C0D75B2180E7AE07DD67CB12EB0` |

`regression-tests-final.log` records the passing client run. The first run exposed
a zero-tolerance test assertion comparing timestamps sampled on either side of a
function call; the assertion now allows 100 ms of scheduling tolerance. This
test-only correction did not change the production deadline. Common test results,
build logs, signature checks and `validation.json` are retained alongside the APK.

## Rollback and remaining checks

The existing native-P2P switch still restores the original connection strategy.
The previous APK is retained under `artifacts/android-native-fixes-20261014/`.
No running client, service, TUN, route or firewall setting was changed for this work.

Android real-device tests remain pending. In particular, cold starts, delayed DNS,
cellular/VPN changes, real picture/input and sustained sessions must be checked on
a phone; the loopback protocol tests and an APK build do not establish those results.
