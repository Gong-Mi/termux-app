#include <signal.h>
#include <ucontext.h>
#include <stddef.h>
#include <stdint.h>

#if defined(__x86_64__) && !defined(REG_RDI)
#define REG_RDI 8
#define REG_RSI 9
#define REG_RDX 12
#endif

// Use offsetof so the compiler always calculates the correct offset
// for the current Android version / NDK headers, regardless of
// struct layout changes (sigset_t size, padding, etc.).

uintptr_t get_execve_path(void *ucontext) {
#if defined(__aarch64__)
    return *(uintptr_t *)((char *)ucontext + offsetof(ucontext_t, uc_mcontext.regs[0]));
#elif defined(__x86_64__)
    return *(uintptr_t *)((char *)ucontext + offsetof(ucontext_t, uc_mcontext.gregs[REG_RDI]));
#else
    return 0;
#endif
}

uintptr_t get_execve_argv(void *ucontext) {
#if defined(__aarch64__)
    return *(uintptr_t *)((char *)ucontext + offsetof(ucontext_t, uc_mcontext.regs[1]));
#elif defined(__x86_64__)
    return *(uintptr_t *)((char *)ucontext + offsetof(ucontext_t, uc_mcontext.gregs[REG_RSI]));
#else
    return 0;
#endif
}

uintptr_t get_execve_envp(void *ucontext) {
#if defined(__aarch64__)
    return *(uintptr_t *)((char *)ucontext + offsetof(ucontext_t, uc_mcontext.regs[2]));
#elif defined(__x86_64__)
    return *(uintptr_t *)((char *)ucontext + offsetof(ucontext_t, uc_mcontext.gregs[REG_RDX]));
#else
    return 0;
#endif
}
