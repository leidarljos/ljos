/* Point TMPDIR at the directory it names, with symlinks resolved.
 * On macOS /var is a symlink of /private/var, and a temp path that
 * still contains that link does not compare equal to the path git
 * prints. This runs only in test binaries.
 */

#include <limits.h>
#include <stdlib.h>

void ljos_test_canon_tmpdir_keep(void) {}

__attribute__((constructor(65534))) static void ljos_test_canon_tmpdir(void) {
    const char *env = getenv("TMPDIR");
    const char *base = (env && env[0]) ? env : "/tmp";
    char buf[PATH_MAX];
    if (!realpath(base, buf)) {
        return;
    }
    setenv("TMPDIR", buf, 1);
}
