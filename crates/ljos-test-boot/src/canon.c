/* Point TMPDIR at the directory it names, with symlinks resolved.
 * On macOS /var is a symlink of /private/var, and a temp path that
 * still contains that link does not compare equal to the path git
 * prints. This runs only in test binaries.
 */

#include <limits.h>
#include <stdlib.h>
#include <string.h>

void ljos_test_canon_tmpdir_keep(void) {}

/* HOME as the process had it before main, symlinks resolved when it
 * exists. A test that points HOME at a temp tree later does not move
 * the seat roots or the git ceilings other tests are using.
 */
static char boot_home[PATH_MAX];

const char *ljos_test_boot_home(void) { return boot_home[0] ? boot_home : NULL; }

__attribute__((constructor)) static void ljos_test_save_home(void) {
    const char *env = getenv("HOME");
    if (!env || !env[0]) {
        return;
    }
    if (realpath(env, boot_home)) {
        return;
    }
    size_t len = strlen(env);
    if (len < sizeof boot_home) {
        memcpy(boot_home, env, len + 1);
    } else {
        boot_home[0] = '\0';
    }
}

__attribute__((constructor(65534))) static void ljos_test_canon_tmpdir(void) {
    const char *env = getenv("TMPDIR");
    const char *base = (env && env[0]) ? env : "/tmp";
    char buf[PATH_MAX];
    if (!realpath(base, buf)) {
        return;
    }
    setenv("TMPDIR", buf, 1);
}
