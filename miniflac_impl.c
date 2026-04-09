/*
 * Compile the miniflac single-header library.
 *
 * MINIFLAC_PRIVATE=static inline tells miniflac to emit all internal
 * functions as static inlines rather than extern symbols, giving the
 * compiler full visibility for inlining/optimization. This yields ~4x
 * decode speed on embedded targets.
 */
#define MINIFLAC_PRIVATE static inline
#define MINIFLAC_IMPLEMENTATION
#include "miniflac/miniflac.h"
