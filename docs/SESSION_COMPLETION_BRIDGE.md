# D2b1: raw completion bridge

Base: PR13, 1ab2e6777edb99095c55625c7a1829d472c36de5.

This slice wires real process/IO terminal facts through JNI into the existing
TerminalSession before or after engine adoption. It does NOT complete the UI,
change exit codes, append a completion banner, or dispose a retained emulator.
D2b2 must separately implement main-thread completion and preserve Activity and
Service removal/transcript policy.

## Ownership and linearization

- SessionRecord retains independent process/IO facts. The non-reused session ID
  identifies this session lifetime; no second generation is introduced.
- A single CompletionSink is installed before the async creator starts either
  process or IO producers. Installation after facts is also supported and tested.
- Installing a sink reserves candidate consumption against the legacy take API.
  Under the registry lock, both facts plus a sink claim the candidate once and
  set dispatch_status=1. The sink is then called OUTSIDE the registry lock.
- dispatch_status: 0 not attempted, 1 in flight, 2 bridge accepted, 3 bridge
  rejected/failed. Missing session returns None/native JNI -1. State 2 is NOT
  Handler.post, client callback, thread join, output drain or final presentation.
- Callback failure is not retried. Rust unwind records failure then resumes
  unwinding; Java errors are cleared on the native reporting thread and recorded
  as rejection. Raw process and IO outcome kinds/codes are not synthesized.
- unregister removes membership first, then drops the removed SessionRecord and
  any sink/global reference outside the registry lock. An already-claimed sink
  may still run; the Kotlin receiver rechecks session identity and disposal.
- The installed sink owns only the stable bridge global reference, not an engine,
  fd or new worker. On dispatch the sink leaves the registry and drops after the
  invocation. Pre-offer creation failure can retain the sink until explicit
  disposal, matching the still-open initialization-failure presentation boundary.

## Kotlin receipt

RustEngineCallback.onSessionCompletion forwards to TerminalSession.
TerminalSession.onNativeCompletion serializes against initialization/disposal;
INITIALIZING is a valid recipient. It stores one immutable CompletionFacts tuple.
Duplicate, mismatched-session, IDLE or DISPOSED delivery returns false.
Client replacement cannot reroute facts to another TerminalSession.

CompletionFacts intentionally remains raw: process exited/lost, IO EOF,
cancelled, errno, response overflow and panic remain distinct. getExitStatus,
isRunning, Handler messages and onSessionFinished are not changed in this slice.
The next UI slice must not treat this raw receipt as completed UI delivery.

## Tests and evidence boundary

Existing registered session_completion_candidate target now covers late sink
installation, early facts, callback registry reentry, failure without retry,
callback unregister and exclusive consumption. Existing process/IO ordering,
standalone session-zero isolation and panic cases remain.

EngineDeliveryNative uses actual production Kotlin, actual JNI library and real
shell processes with a controlled Handler/Message/Log queue (NOT ART). New cases
observe native exit+EOF before adoption, raw facts surviving adoption, duplicate
and stale rejection, post-disposal rejection, and a real JNI false result from
an unbound receiver. Existing adoption/disposal cases remain in the same mode.

Initial Rust RED: eight E0599 missing bridge APIs. GREEN: registered target and
real Kotlin/JNI harness executed locally. Exact-head CI and its original emulator
artifacts must be recorded separately; this document does not assert CI success.
