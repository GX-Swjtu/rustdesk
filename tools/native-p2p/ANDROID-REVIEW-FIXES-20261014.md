# Android registration review fixes, 2026-10-14

This delivery combines the reviewed default-on Android integration, the PC/Android
graphical switch and two fixes from `artifacts/android-p2p-review-20261014/REVIEW.md`.
It is based on client `bea054765` and common `ce2570ba`. Client common now pins
`90015d4ff0875f1153d111245088869e5b1bd34d`; the server common baseline is unchanged.

## Repairs

- A controller without a sharing mediator starts with the original `RegisterPeer`
  exchange. It sends `RegisterPk` only when hbbs replies with `request_pk=true`,
  then publishes a freshly signed capability. Rapid reconnects reuse the server's
  registered key and do not consume its public-key registration quota each time.
  No new key cache, retry loop or server-side exception was added.
- Android temporary registration and the signed Begin RPC share a one-second
  timeout. DNS, UDP registration and TCP capability negotiation all consume that
  budget. Only a negotiated native session continues into the remaining portion
  of the existing 45-second attempt deadline. Failure and cancellation retain
  registration withdrawal and release of the attempt slot.

These repairs are local to `src/native_p2p/android_registration.rs`, its Android
export and the Android branch of `src/client/native_p2p.rs`. The Windows/Linux
admission path still calls the existing `Session::begin` directly.

## Verification

- Final Windows host suite: **7 passed, 0 failed** (`cargo test --locked --offline
  --release --lib --features flutter,hwcodec native_p2p -- --test-threads=1`).
  The rapid-reconnect test completes four registrations/withdrawals while sending
  one public key, against a fixture with hbbs's key-registration rate limit.
  The compatibility test exercises both 600 ms and 1500 ms legacy UDP delays
  with an old TCP endpoint that does not answer the native protocol. Each attempt
  returns within 1300 ms (one-second deadline plus scheduler tolerance), and its
  attempt slot is subsequently released. Existing bypass, KCP handoff, default-on,
  LAN bounds and option-cancellation tests also pass.
- Final Android arm64 Rust release compilation and debug APK assembly: **passed**,
  using the existing locked/offline dependencies and toolchains. The dependency
  tree is unchanged from the initial Android default-on build.
- APK signature verification: **passed**, v1/v2. Metadata: RustDesk 1.5.1 (2068),
  min SDK 22, target SDK 36; arm64-v8a is the only packaged ABI. The embedded Rust
  library matches the final compiled library. Its `Java_ffi_FFI_initNativeP2p`
  export is present. Existing compiler/linker and Gradle deprecation warnings
  remain; no unrelated warning cleanup was made.
- `git diff --check` passed for client and common. The common gitlink includes
  exactly one commit with three platform/documentation changes. The server
  checkout and its common submodule remain clean.

Artifacts and complete logs are in `artifacts/android-native-fixes-20261014/`.

| Artifact | SHA-256 |
|---|---|
| `rustdesk-native-p2p-android-arm64-review-fixes.apk` | `24efeba48a073739ac82ea248dbe8f2768b160ec343956a10a2d292f2c8d3555` |
| Compiled and APK-embedded Rust library | `88640b18221c75657083a489a37ffa1cb8c763f591aa599fc7146de018ae0c63` |

`source-manifest.json`, `validation.json`, the retained build script and package
verification script record the inputs and checks. The APK is a debug test build;
it was not installed on a device or deployed to an existing client.

## Scope and rollback

The complete list of changed existing runtime paths and their necessity is in
[ANDROID.md](ANDROID.md#existing-paths-touched). The minimization pass confirmed:

- Common changes only enable the existing crate/re-export on Android and update
  its platform documentation. No algorithm, protocol or dependency version changes.
- Client connection/invitation and mediator hooks admit Android to the feature;
  option and session hooks cancel pending native work. Legacy fallback remains
  the original implementation.
- `MainApplication.kt` and `ffi.kt` initialize and expose optional read-only LAN
  discovery. `MainActivity`, `MainService`, Android permissions and SDK settings
  have no diff; no Activity, Wi-Fi or stability prerequisite was added.
- Flutter constants and desktop/mobile settings expose the existing default-on
  option. PC uses General > Other and the existing service-synchronized checkbox.
  No Network-page unlock or new administrator API was added.
- No hbbs/hbbr code, production/test service, installed client, firewall, route
  or TUN configuration changed during these repairs. Android JNI library copies
  and packages are build artifacts only.

Disable **Enable UDP hole punching (Native P2P beta)** to use the original
connection strategy. An existing saved `N` remains respected. Package rollback
uses the previous test APK with its matching signing key; keep its configuration.

No Android device is connected. Actual Android picture/input, both connection
directions, VPN/cellular/background behavior, battery use and sustained resource
cleanup remain device-validation tasks. Host tests and successful APK compilation
do not replace those checks. Existing Windows UI-package evidence is retained in
`artifacts/native-p2p-general-ui-20261014/`; these Android-only repairs do not change
that graphical switch or its Windows runtime implementation.
