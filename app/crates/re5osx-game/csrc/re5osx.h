/* Glue between the wasm2c output and the Rust host. */
#ifndef RE5OSX_H
#define RE5OSX_H

#include <setjmp.h>
#include <stdint.h>

#include "wasm-rt.h"

/* Trap targets without the signal mask: no signal handler ever jumps to
 * one (bounds are checked in the code, no guard pages), and saving it is a
 * system call at every entry into wasm (natives, hooks calling the engine:
 * ~170 ns each). siglongjmp takes a buffer saved either way. */
#ifndef _WIN32
#undef WASM_RT_SETJMP_TRAP_SETBUF
#define WASM_RT_SETJMP_TRAP_SETBUF(buf) sigsetjmp(buf, 0)
#endif

/* One per wasm instance, i.e. per engine thread. Every import and hook
 * receives it. Mirrored by `re5osx_game::Env`. */
struct w2c_env {
  wasm_rt_shared_memory_t* memory; /* the module's (single, shared) memory */
  struct w2c_game* instance;       /* this thread's instance */
  void* host;                      /* host data for this thread */
  void* longjmp_target;            /* innermost invoke_* frame (jmp_buf*) */
};

/* The WASI imports get the same object. */
struct w2c_wasi__snapshot__preview1 {
  struct w2c_env env;
};

/* Lightweight setjmp for Emscripten's setjmp/longjmp emulation: no signal
 * mask save/restore. On Windows, no unwinding either: the CRT's longjmp
 * unwinds like an exception through the frames in between (the host's
 * among them, and Rust aborts there) unless the jmp_buf has no frame
 * (see patches/wabt/0006, the same for traps). */
#if defined(_WIN32) && (defined(_M_X64) || defined(_M_ARM64))
#define RE5OSX_SETJMP(b) setjmp(b)
#define RE5OSX_LONGJMP(b, v) (((_JUMP_BUFFER*)(b))->Frame = 0, longjmp(b, v))
#elif defined(_WIN32)
#define RE5OSX_SETJMP(b) setjmp(b)
#define RE5OSX_LONGJMP(b, v) longjmp(b, v)
#else
#define RE5OSX_SETJMP(b) _setjmp(b)
#define RE5OSX_LONGJMP(b, v) _longjmp(b, v)
#endif

/* Run `stmt` (a call into wasm through `env`) and return the trap code
 * (0 = none). Nested calls (wasm -> host -> wasm) keep the outer trap
 * target intact; after a trap the wasm stack pointer and the innermost
 * setjmp frame are put back (the frames skipped never popped theirs: a
 * caller that goes on would otherwise longjmp into a dead frame). */
#define RE5OSX_TRY(stmt)                                                   \
  do {                                                                    \
    uint64_t re5osx_sp_ = env->instance->w2c_0x5F_stack_pointer;           \
    void* re5osx_longjmp_target_ = env->longjmp_target;                    \
    wasm_rt_jmp_buf re5osx_saved_ = g_wasm_rt_jmp_buf;                     \
    wasm_rt_trap_t re5osx_trap_ = wasm_rt_impl_try();                      \
    if (re5osx_trap_ == WASM_RT_TRAP_NONE) {                               \
      stmt;                                                               \
    } else {                                                              \
      env->instance->w2c_0x5F_stack_pointer = re5osx_sp_;                  \
      env->longjmp_target = re5osx_longjmp_target_;                        \
    }                                                                     \
    g_wasm_rt_jmp_buf = re5osx_saved_;                                     \
    return (uint32_t)re5osx_trap_;                                         \
  } while (0)

/* memory.atomic.wait/notify, provided by the host (see futex.rs). */
typedef uint32_t (*re5osx_wait32_fn)(void* memory, uint64_t addr, uint32_t expected, int64_t timeout_ns);
typedef uint32_t (*re5osx_wait64_fn)(void* memory, uint64_t addr, uint64_t expected, int64_t timeout_ns);
typedef uint32_t (*re5osx_notify_fn)(void* memory, uint64_t addr, uint32_t count);

/* Implemented in re5osx.c. */
void re5osx_set_atomics(re5osx_wait32_fn wait32, re5osx_wait64_fn wait64, re5osx_notify_fn notify);
uint32_t re5osx_call_ptr_ptr(struct w2c_env* env, uint64_t index, uint64_t arg, uint64_t* out);
uint32_t re5osx_call_native(struct w2c_env* env, uint64_t index, uint64_t info);
void re5osx_init(void);
void re5osx_memory_init(wasm_rt_shared_memory_t* memory,
                       uint8_t* data,
                       uint64_t pages,
                       uint64_t max_pages);
size_t re5osx_memory_struct_size(void);
uint32_t re5osx_instance_new(struct w2c_env* env);
void re5osx_set_host_functions(const void* const* fns, uint32_t count);
extern uint32_t re5osx_host_functions_base;
void re5osx_instance_free(struct w2c_env* env);
_Noreturn void re5osx_longjmp(struct w2c_env* env);

/* Trap code of re5osx_unwind(). */
#define RE5OSX_TRAP_UNWIND 0x7f000001
/* Unwinds to the innermost call into wasm, which returns RE5OSX_TRAP_UNWIND
 * (emscripten_exit_with_live_runtime). */
_Noreturn void re5osx_unwind(void);

#endif
