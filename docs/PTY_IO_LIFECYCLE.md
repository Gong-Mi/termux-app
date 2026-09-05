# PTY IO lifecycle construction contract

Base: d75b67f7dcd373d4f3f41136541bee67e549c290 (PR8).
This is the B slice, not process-exit/UI completion or Surface-generation work.

## Complete production boundary

- engine/context.rs::start_io_thread currently consumes a reader duplicate,
  blocks in read and separately writes parser responses through atomic raw fd.
  Its EOF retry discards a successful nonzero second read.
- JNI RustTerminal.processInput independently writes that raw fd, ignoring
  partial/error results. startIoThread starts a reader; createSessionAsync owns
  original fd and creates a duplicate. engine::destroy_engine closes the raw
  original concurrently with these users and does not join the reader.
- TerminalSession.write invokes processInput; updateSize uses a separately
  cached raw fd via JNI.setPtyWindowSize. Both must migrate together.
- TerminalEmulator new constructor passes a PTY to startIoThread; adoption must
  continue to avoid creating an extra engine or starting another reader.
- Synchronous JNI.createSubprocess/setPtyWindowSize/close are legacy raw-FD API;
  their external ABI is not globally redesigned here. The production async
  session stops using cached raw-fd resize. Process signaling/reaping is C.

## Intended contract

One worker owns the PTY OwnedFd and performs all PTY read/write/ioctl syscalls.
O_NONBLOCK applies to the shared open-file-description, so all production
writers move in the same batch. Poll observes PTY plus a nonblocking wake fd.
Destroy revokes admission and wakes the worker; join is off the calling/UI thread.
A callback/engine lock can still delay completion: no claim of bounded JNI
callback execution or forced thread termination.

A bounded 1 MiB pending-byte budget includes in-flight user data and parser
responses. Input enqueue is all-or-none with explicit accepted/closed/full/invalid
status. New tryProcessInput exposes the status; legacy processInput JVM signature
is preserved and logs rejection. TerminalSession reports rejection via client
warning/bell rather than silently treating a rejected paste as delivered. No
unbounded fallback, blocking main-thread write, or automatic duplicate retry.
Queue capacity and rejection behavior are an explicit policy change, not a
behavior-equivalent performance optimization. Accepted means queued, not delivered;
forced cancellation may discard accepted pending bytes and reports cancellation.

Partial writes retain offsets; EINTR retries, EAGAIN waits for POLLOUT. Parser
responses use the same FIFO; inability to queue a response is an explicit failure
outcome, not a dropped response hidden behind success. Worker read/write work is
bounded per poll turn to avoid cancellation starvation. Pending resize requests
may coalesce to the latest dimensions, and execute on the fd owner thread.

Natural EOF/error, forced cancellation, parser-response overflow, and delivery
completion are distinct. No exit callback is wired here. No fake EOF debounce
read may discard accepted bytes. No stale raw fd can close/write a recycled fd.

## Acceptance to implement and run

- Actual PTY silent slave cancellation; blocked/saturated writes; exact ordering
  and partial writes; bounded full rejection; repeated close and descriptor reuse.
- Start failure and repeated start do not leak or replace the active worker.
- Production context parsing/response/resize integration and cancellation join.
- Real Kotlin/JNI existing handle tests plus new input-status and shutdown cases.
- Four correctness tiers, strict changed-code checks, full ABI and emulator CI.
- Distinguish source/native host/JVM/ART/GPU. Preserve existing inherited Clippy
  and other test debts rather than weakening their assertions.

At initial contract commit no implementation or runtime success is claimed.

## Implemented local evidence (before exact-head CI)

- Production runtime/context/JNI/Kotlin now use the owner handoff above. Normal
  transport terminal reasons are logged immediately after fd closure; background
  join remains a distinct completion event. Callback panic is observed by join.
- Parsing, reply admission, and Java notifications are separate worker phases:
  responses enter FIFO before callback reentrant input. Screen notifications
  retain the old before-bell/color/clipboard ordering. This does NOT retain the
  old blocking write-before-callback syscall timing; callback latency may still
  delay queued transmission. No bounded foreign-callback completion claim.
- Actual Rust runtime 12 tests, production context 4, global ownership integration
  1 pass. Includes real PTY initial slave open, silent/write-saturated cancel,
  large exact input/reply ordering, explicit full/overflow, callback cancellation
  boundary/panic, isolated-process fd reuse, real resize, EOF and failed fd setup.
- Real production Kotlin + JNI: 43 handle methods, invalid/revoked ownership,
  actual shell input split across legacy/status methods, offset/count/invalid/full
  statuses, and handle-based resize verified using child `stty size` all pass.
  Constructor recording 3 adoption/12 creation cases and real native adoption,
  parsing/resize/destroy pass. Host JVM is not ART/UI acceptance.
- `cargo check --tests --locked --offline` passes for the test compilation surface;
  pre-existing warnings remain. Python discovery 35 tests and both IO/handle
  static boundary gates pass. CI wiring contracts pass.
- core/regressions/terminal pass. lifecycle completes all targets but old
  std-only `test_frame_backpressure_simulation` fails (submitted12/blocked13);
  file unchanged from base, no assertion weakened. Initial broader run lacked
  local libc++ linker search path; rerun with existing NDK r23 LIBRARY_PATH reached
  real tests. Neither link setup failure nor this old oracle is a PTY regression.
- Independent read-only review found no new blocker on repository's legitimate
  production calls. Parent subsequently separated reply/notification ordering,
  added immediate normal outcome observation, fd-setup failure test and real JNI
  resize. Exact same raw fd must not be ownership-transferred twice: the legacy
  API retains that precondition, distinct-owned-fd repeated starts are rejected.

Not yet verified: deterministic EINTR/EAGAIN and spawn/reaper-failure injection;
ART rejection UI, device lifecycle/GPU, whole process-exit/descendant-drain policy.
Socket half-close is not a supported drain guarantee: EOF discards queued output
by contract. Subagent's missing-module RED only proves initial interface absence;
the full behavior suite is parent-executed integration/characterization, not
fabricated retrospective TDD. Exact-head CI remains required after publication.
