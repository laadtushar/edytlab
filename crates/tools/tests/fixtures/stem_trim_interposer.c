/* Test-only LD_PRELOAD interposer.  It records explicit malloc_trim calls
 * without touching the allocator, so the lifecycle test needs no RSS limit. */
#include <stdatomic.h>
#include <stddef.h>

static _Atomic unsigned long trim_calls = 0;

int malloc_trim(size_t pad) {
    (void)pad;
    atomic_fetch_add_explicit(&trim_calls, 1, memory_order_relaxed);
    return 0;
}

unsigned long stem_test_trim_calls(void) {
    return atomic_load_explicit(&trim_calls, memory_order_relaxed);
}
