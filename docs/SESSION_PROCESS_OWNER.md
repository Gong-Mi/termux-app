# Known-child process ownership: C1 contract

Base PR9: 48f32866f85dee4474ccd83b8586cd0c3f44e3fd.

## Confirmed RED on real child processes

The existing production coordinator failed all four new isolated-process tests:
- waitpid(-1) steals an unrelated child's exit status (its real owner gets ECHILD).
- exit before bind is discarded; late bind writes Running forever.
- late bind resurrects an unregistered session.
- releasing pkg lock after exit overwrites Finished with Running.

These failures were actually run against the old implementation, not inferred
from test names. Sleep windows expose the old monitor race; they are not proofs
of every possible scheduler ordering.

## Scope and complete boundary

- Remove coordinator's wildcard/global reaper. Claim and retain a ProcessOwner
  only for a positive, actually waitable child PID. Bind before starting its
  monitor; already-exited status is retained in the owner rather than discarded.
- Linux/Android pidfd is preferred for process identity/signaling and readiness.
  Kernel/SELinux unavailable paths use same-state-lock WNOHANG waitpid and kill,
  with bounded timed polling rather than holding a lock across blocking waitpid.
  Never fall back to raw kill after a pidfd signal failure. ECHILD revokes signal
  authority. Foreign code bypassing the fallback reaper lock remains outside its
  identity guarantee; do not claim universal PID reuse safety on old kernels.
- Retain normal/signal exit or explicit lost-ownership status. Multiple managed
  waiters observe the same terminal result, never race to reap the same child.
- Registered session membership gates bind/state publication. Finished is
  absorbing; unregister cannot be resurrected by a late monitor or pkg release.
  Binding a live child twice is rejected before a second owner may reap it.
- Kotlin finishIfRunning routes through stable native session ID, not mShellPid.
  Termination requested before bind is retained and applied to the eventual
  owner. Process identity/termination is separate from IO cancellation.
- Async engine context retains process owner so new input is rejected after
  known process exit while the IO reader may still consume descendant-held tail.
- Managed legacy waitFor looks up the existing owner instead of competing with
  its monitor. Unknown legacy raw-PID users retain their own ownership contract;
  no promise of stale-PID identity after that API loses its owner.
- The existing PTY active-child counter must decrement once for managed async
  children as well as legacy synchronous waits; tests spawning unrelated children
  must not decrement that accounting.

## Deferred, not accidentally enabled

No onProcessExited/MSG_PROCESS_EXITED callback is connected in C1. Native process
exit, reader EOF, output drain, callback delivery, final-screen retention and
session disposal remain separate. Java mShellPid/isRunning/UI completion is not
fully migrated here; removing raw-PID kill prevents that stale presentation value
from becoming signal authority. C2/D handles delivery and UI state together.

No IO timeout policy, arbitrary grace period, Surface generation, global signal
handler, device reboot or automatic PR merge. Keep existing tests and gates.

## Acceptance

Actual-child RED→GREEN above; normal/signal exits and pre-bind exit; concurrent
wait/kill with cached result; pidfd and forced fallback; invalid/nonchild reject;
late unregister/bind/exit and pkg lock; pending terminate; real Kotlin/JNI shell
termination/status/retained output; old IO/handle/constructor suites; complete
correctness tiers and four ABI/emulator CI. Tests do not prove real PID-number
reuse or arbitrary unmanaged third-party reapers safe.

## Implementation map (C1 only)

- `process_owner.rs`: claim/claim_fallback validate a positive waitable child;
  wait_pidfd/refresh cache Exited or Lost; terminate refreshes under the same
  mutex before signaling; wait releases that mutex before poll/condvar waits.
  EPERM/EINVAL/ENOSYS from P_PIDFD waiting switch only the wait mechanism to
  WNOHANG waitpid, retaining any pidfd for signaling. ECHILD is not downgraded.
  No pidfd signal error falls back to raw kill.
- `coordinator.rs`: Registry owns membership/pkg state/PID weak-owner lookup;
  register_session allocates non-reused signed-JNI-compatible IDs;
  bind_pid/bind_pty_child/bind_child reject duplicates before claim, retain
  pre-bind termination, and start one known-child monitor. process_exited checks
  owner identity before setting Finished; unregister_session removes membership
  and revokes pending delivery. terminate_session never uses a Java PID.
  process_status provides pending/running/exited/lost without a terminal kill PID.
  set_engine_data/take_engine_data reject late poll publication and reclaim the
  handle; push/UI acknowledgement remains a later delivery contract.
  pkg acquire/release cannot resurrect Finished. managed_process_for_pid lets
  legacy waitFor share an existing managed outcome instead of reaping twice.
- `pty.rs`: managed async monitor decrements active-child accounting once;
  generic bind_pid test children do not decrement it. Legacy unknown raw-PID
  wait_for retains its inherited caller-ownership/accounting limitation.
- `engine/context.rs`: with_process retains the owner; submit_input rejects a
  cached known exit without cancelling reader tail. New process exit can race
  with a call that already passed admission; accepted remains not delivered.
- `jni/terminal_emulator.rs`: async create checks membership, transfers the child
  to bind_pty_child once, and gives its owner to TerminalContext. Rejected bind
  cleanup is not repeated using a numeric PID after owner release.
- `JNI.kt`/coordinator JNI: terminateSession and getSessionProcessStatus added;
  old descriptors retained. `TerminalSession.kt` stores mNativeSessionId and
  routes finishIfRunning through it, not Os.kill(mShellPid).
