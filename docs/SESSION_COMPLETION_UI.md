# D2b2: main-thread completion and retained terminal results

Base PR14: 7f5c09cf656110cee58f93fa540989ff9921288d.
This extends the raw-receipt bridge documented in SESSION_COMPLETION_BRIDGE.md.

## State and ownership

TerminalSession raw receipt stores immutable process/IO facts as before.
CompletionDelivery then progresses NONE -> PENDING -> POSTED -> RUNNING ->
ATTEMPTED -> DELIVERED/FAILED. PENDING waits for actual engine claim/ack/READY;
POSTED is Handler queue admission only; RUNNING means preparing terminal output;
ATTEMPTED means entering onSessionFinished. A rejected/throwing post is FAILED.
Disposal cancels pending/posted work, and dequeued stale work cannot resurrect it.
No automatic retry is added. Native dispatch status=2 remains raw bridge receipt,
not UI completion.

On the main-thread runnable, the session publishes terminal process presentation
state, appends the completion banner after parsed tail, and requests screen update.
Client callbacks run outside lifecycle/native registry locks; text notification
failure does not suppress the separate result callback. A callback exception is
recorded, not retried. A client can synchronously capture output and dispose.

The old disconnected MSG_PROCESS_EXITED/cleanupResources path is removed: it
would have destroyed the emulator before the result callback. The new path does
not destroy the emulator or close the queues. Explicit removal/dispose remains
the final engine owner boundary. Retention proves getters/transcript still work;
it does not prove the final GPU frame was presented.

## Process and transport results

- Exited: getProcessExitStatus returns the actual signed status (including signal
  encoding already used by ProcessOwner). Raw IO errors remain independent.
- Lost: getProcessExitStatus returns null. Legacy getExitStatus returns presentation
  sentinel 1 to avoid treating an unknown exit as success; ResultData never exports
  that sentinel as a real process status.
- getCompletionError reports Lost and/or cancelled, IO errno, response overflow or
  panic. Cancellation is not full drain. Both independent errors are retained.
- TermuxSession.finish captures actual nullable status and transcript first. A
  completion error sets ExecutionCommand FAILED using its existing error channel,
  even when actual process status is zero. No arbitrary shell exit code is invented.
- shouldNotProcessResults is a consuming latch, not a pure getter; only the existing
  processTermuxSessionResult routine owns it. The new finish path must not consume
  it before result delivery.

## Product policy

The normal completion banner remains before transcript capture, preserving the
old ordering (plugin stdout may contain the banner). This is not a stdout protocol
cleanup.

Foreground non-TV sessions still auto-remove on normal exit 0/130, except a newly
observable transport/unknown-status error is retained rather than hidden as
success. Plugin pending results still remove immediately and report the failure.
TV's existing session-count/plugin conditions are unchanged.

The Service-only client now fulfils pending plugin results via finish when no
Activity is attached. Ordinary Service-only sessions remain retained for later
attachment. Existing Service removal reads/processes results before dispose.

## Executed tests and limits

Initial RED used actual Kotlin/JNI/real shell: terminal facts arrived but the
main-thread client finished count remained zero. GREEN uses the same path and
asserts tail+banner are readable in the callback, exactly one callback, process
status and retained emulator.

The real JNI queue harness also checks client replacement before delivery, actual
ProcessOwner kill, post(false), dequeued-after-dispose, throwing text/finished
clients, and callback-driven capture then disposal. Abnormal raw outcome cases
are explicitly injected at the receiver boundary, not claimed as actual kernel
IO failures or external-reaper reproduction.

verify-completion-result.py compiles exact extracted production finish/result and
Service callback methods, plus the actual consuming-latch method, against explicit
Java dependency fixtures. It exercises null Lost status, zero+IO failure, normal
codes, prior failure, running guard, plugin-only Service dispatch and transcript
before simulated removal. It is method-level evidence, NOT Android Service or
full ExecutionCommand integration. Exact-head APK compilation and subsequent
ART/device lifecycle verification are separate gates.

Outstanding boundaries: full ART Service/plugin lifecycle, real final-frame
presentation, callback quiescence after an already-started invocation, unavailable
Looper delivery recovery, initialization failure presentation, and a child or
descendant retaining the PTY slave. No new drain timeout, Surface protocol, process
termination policy, or Rust producer change is introduced.
