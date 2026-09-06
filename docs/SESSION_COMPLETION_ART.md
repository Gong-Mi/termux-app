# Completion ART instrumentation acceptance

Base PR15 implements completion delivery; this slice adds runtime acceptance, not
new production session behavior.

The app androidTest APK runs SessionCompletionArtTest in the real target process
using AndroidJUnitRunner, actual Handler/Looper, JNI, PTY, production
ExecutionCommand/TermuxSession, bound TermuxService and PendingIntent broadcasts.
No desktop Handler shim or extracted Java-method fixture is used here.

Cases:

1. Hold a real main-thread task while native early-exit facts arrive, proving
   adoption is still pending. After returning to Looper, assert main-thread
   completion once, status 37, readable tail/banner, retained emulator, resize
   and explicit final disposal.
2. Call the production TermuxSession.execute API with a system-shell environment,
   then production finish/ExecutionCommand. Assert actual result status/stdout
   exists before the result callback disposes the emulator.
3. Bind the real TermuxService with no Activity client, create a plugin command
   with mutable package-scoped PendingIntent, receive its actual result broadcast,
   and verify stdout/status plus removal from the service list and emulator
   disposal. The receiving component is in the instrumentation target package;
   this is not external untrusted-plugin permission enforcement testing.

Existing SwiftShader same-APK Skia A/B stays intact. After A/B, the workflow
installs the test APK and runs all three cases on the same emulator. The driver
requires ro.kernel.qemu=1 before installs/stops, verifies checkout/build identity,
records the test APK hash, instrumentation output, logcat and explicit summary.
Exactly the expected test names/count must pass; skip/failure/abort/empty output
cannot pass merely because adb exits zero.

Local Python tests use clearly synthetic instrumentation transcripts only to
validate the verifier's pass/failure/count behavior and registration. They are
NOT Android execution evidence. Test APK compilation and real ART test outcomes
must be taken from the exact-head CI artifacts.

Still outside this slice: physical GPU performance, final rendered terminal-text
pixels, Surface-generation races, external plugin security/permission handling,
and a full Activity auto-removal policy matrix. Retained terminal text and a
separate A/B first-frame result do not prove the final completion text was presented.
