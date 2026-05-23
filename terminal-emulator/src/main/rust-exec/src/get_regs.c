#include <signal.h>
#include <ucontext.h>

// C 编译器会自动根据当前平台的 ucontext_t 布局算出正确偏移
unsigned long get_execve_path(void *ucontext) {
#if defined(__aarch64__)
    return ((ucontext_t *)ucontext)->uc_mcontext.regs[0];
#elif defined(__x86_64__)
    return ((ucontext_t *)ucontext)->uc_mcontext.gregs[REG_RDI];
#else
    return 0;
#endif
}

unsigned long get_execve_argv(void *ucontext) {
#if defined(__aarch64__)
    return ((ucontext_t *)ucontext)->uc_mcontext.regs[1];
#elif defined(__x86_64__)
    return ((ucontext_t *)ucontext)->uc_mcontext.gregs[REG_RSI];
#else
    return 0;
#endif
}

unsigned long get_execve_envp(void *ucontext) {
#if defined(__aarch64__)
    return ((ucontext_t *)ucontext)->uc_mcontext.regs[2];
#elif defined(__x86_64__)
    return ((ucontext_t *)ucontext)->uc_mcontext.gregs[REG_RDX];
#else
    return 0;
#endif
}
