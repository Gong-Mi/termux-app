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
