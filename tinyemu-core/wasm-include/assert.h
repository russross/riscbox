#ifndef TINYEMU_ASSERT_H
#define TINYEMU_ASSERT_H

_Noreturn void tinyemu_assert_fail(const char *expression, const char *file, int line);
#define assert(expression) ((expression) ? (void)0 : tinyemu_assert_fail(#expression, __FILE__, __LINE__))

#endif
