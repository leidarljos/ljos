/* Linked only into test binaries that depend on ljos-test-boot.
 *
 * A test models the identity it builds. The host cgroup is not that
 * identity: a cloud runner's named workload cgroup is that runner,
 * and a transient run-*.scope is not a login. This file
 * moves the test process into a cgroup that names neither, before main,
 * and drops runner variables the host left behind. Children inherit the
 * cgroup. There is no executable-path check. The product binary does
 * not link this object, so it never moves itself.
 *
 * Entering a runner cgroup is the other way around. A fresh directory is
 * required when the process sits at the namespace root (0::/). Reusing
 * /sys/fs/cgroup/claude fails once that directory has children, and a
 * domain-invalid child rejects cgroup.procs.
 */

#ifdef __linux__

#define _GNU_SOURCE

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

/* Shipped harness names. A cgroup word matches one of these. Short names
 * and session prefixes are rejected by word_counts, as in the library. */
static const char *const RUNNERS[] = {
    "opencode", "opencode-v2", "hermes",   "claude",    "codex",     "antigravity",
    "cursor",   "grok",        "grokbot",  "windsurf",  "vscode",    "claude-desktop",
    "gemini",   "amazonq",     "kiro",     "copilot",   "factory",   "qwen",
    "crush",    "cline",       "auggie",   "goose",     "openhands", "continue",
    "kilo",     "runner-with-a-command", "runner-with-a-config-file",
    "runner-with-a-json-config",
};

static const char *const WRAPPERS[] = {
    "sh",   "bash", "zsh",   "fish",   "dash",   "ksh",    "tcsh",
    "csh",  "nu",   "env",   "sudo",   "doas",   "timeout","nohup",
    "xargs","script","setsid","uv",    "direnv", "ljos",   "ljos-mcp",
};

static const char *const SESSION_PREFIX[] = {
    "tmux", "screen", "zellij", "herdr", "systemd", "init", "sshd", "login",
};

extern char **environ;

static int eq_ci(const char *a, const char *b) {
    while (*a && *b) {
        char ca = *a;
        char cb = *b;
        if (ca >= 'A' && ca <= 'Z') {
            ca = (char)(ca - 'A' + 'a');
        }
        if (cb >= 'A' && cb <= 'Z') {
            cb = (char)(cb - 'A' + 'a');
        }
        if (ca != cb) {
            return 0;
        }
        a++;
        b++;
    }
    return *a == 0 && *b == 0;
}

static int starts_ci(const char *text, const char *prefix) {
    while (*prefix) {
        char ca = *text;
        char cb = *prefix;
        if (ca == 0) {
            return 0;
        }
        if (ca >= 'A' && ca <= 'Z') {
            ca = (char)(ca - 'A' + 'a');
        }
        if (cb >= 'A' && cb <= 'Z') {
            cb = (char)(cb - 'A' + 'a');
        }
        if (ca != cb) {
            return 0;
        }
        text++;
        prefix++;
    }
    return 1;
}

static int word_counts(const char *word) {
    size_t n = strlen(word);
    size_t i;
    if (n < 4) {
        return 0;
    }
    for (i = 0; i < sizeof WRAPPERS / sizeof WRAPPERS[0]; i++) {
        if (eq_ci(word, WRAPPERS[i])) {
            return 0;
        }
    }
    for (i = 0; i < sizeof SESSION_PREFIX / sizeof SESSION_PREFIX[0]; i++) {
        if (starts_ci(word, SESSION_PREFIX[i])) {
            return 0;
        }
    }
    return 1;
}

static int word_is_runner(const char *word) {
    size_t i;
    if (!word_counts(word)) {
        return 0;
    }
    for (i = 0; i < sizeof RUNNERS / sizeof RUNNERS[0]; i++) {
        if (eq_ci(word, RUNNERS[i])) {
            return 1;
        }
    }
    return 0;
}

static int segment_names_runner(const char *segment) {
    char buf[256];
    size_t n = 0;
    const char *p = segment;
    while (*p) {
        char c = *p++;
        int alnum = (c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z');
        if (alnum) {
            if (n + 1 < sizeof buf) {
                buf[n++] = c;
            }
            continue;
        }
        if (n > 0) {
            buf[n] = 0;
            if (word_is_runner(buf)) {
                return 1;
            }
            n = 0;
        }
    }
    if (n > 0) {
        buf[n] = 0;
        if (word_is_runner(buf)) {
            return 1;
        }
    }
    return 0;
}

static int transient_segment(const char *segment) {
    size_t n = strlen(segment);
    if (n <= 6 || strcmp(segment + n - 6, ".scope") != 0) {
        return 0;
    }
    if (n - 6 <= 4) {
        return 0;
    }
    return strncmp(segment, "run-", 4) == 0;
}

static int cgroup_is_foreign(const char *text) {
    char line[1024];
    const char *p = text;
    while (*p) {
        size_t n = 0;
        const char *path;
        const char *seg;
        while (*p && *p != '\n' && n + 1 < sizeof line) {
            line[n++] = *p++;
        }
        if (*p == '\n') {
            p++;
        }
        line[n] = 0;
        path = strrchr(line, ':');
        path = path ? path + 1 : line;
        seg = path;
        while (*seg) {
            char part[256];
            size_t k = 0;
            while (*seg && *seg != '/') {
                if (k + 1 < sizeof part) {
                    part[k++] = *seg;
                }
                seg++;
            }
            if (*seg == '/') {
                seg++;
            }
            part[k] = 0;
            if (k == 0) {
                continue;
            }
            if (transient_segment(part) || segment_names_runner(part)) {
                return 1;
            }
        }
    }
    return 0;
}

static int sudo_run(char *const argv[], int stdin_fd) {
    pid_t pid = fork();
    int status = 1;
    if (pid < 0) {
        return -1;
    }
    if (pid == 0) {
        if (stdin_fd >= 0) {
            dup2(stdin_fd, STDIN_FILENO);
        }
        int sink = open("/dev/null", O_WRONLY);
        if (sink >= 0) {
            dup2(sink, STDOUT_FILENO);
            if (sink > 2) {
                close(sink);
            }
        }
        execvp(argv[0], argv);
        _exit(127);
    }
    if (waitpid(pid, &status, 0) < 0) {
        return -1;
    }
    if (WIFEXITED(status) && WEXITSTATUS(status) == 0) {
        return 0;
    }
    errno = EPERM;
    return -1;
}

static int mkdir_p(const char *dir) {
    char *argv[6];
    if (mkdir(dir, 0755) == 0 || errno == EEXIST) {
        return 0;
    }
    argv[0] = "sudo";
    argv[1] = "-n";
    argv[2] = "mkdir";
    argv[3] = "-p";
    argv[4] = (char *)dir;
    argv[5] = NULL;
    return sudo_run(argv, -1);
}

static int delegate(const char *dir) {
    char spec[64];
    const char *tails[] = {"", "/cgroup.procs", "/cgroup.threads", "/cgroup.subtree_control",
                           "/cgroup.type"};
    size_t i;
    snprintf(spec, sizeof spec, "%u:%u", (unsigned)getuid(), (unsigned)getgid());
    for (i = 0; i < sizeof tails / sizeof tails[0]; i++) {
        char path[PATH_MAX];
        char *argv[6];
        int n = snprintf(path, sizeof path, "%s%s", dir, tails[i]);
        if (n < 0 || (size_t)n >= sizeof path) {
            errno = ENAMETOOLONG;
            return -1;
        }
        if (chown(path, getuid(), getgid()) == 0) {
            continue;
        }
        argv[0] = "sudo";
        argv[1] = "-n";
        argv[2] = "chown";
        argv[3] = spec;
        argv[4] = path;
        argv[5] = NULL;
        if (sudo_run(argv, -1) != 0) {
            return -1;
        }
    }
    return 0;
}

static int write_fd(const char *path, const char *text) {
    int fd = open(path, O_WRONLY | O_CLOEXEC);
    size_t n;
    ssize_t w;
    int err;
    if (fd < 0) {
        return -1;
    }
    n = strlen(text);
    w = write(fd, text, n);
    err = errno;
    close(fd);
    if (w < 0 || (size_t)w != n) {
        errno = w < 0 ? err : EIO;
        return -1;
    }
    return 0;
}

static int write_via_sudo(const char *path, const char *text) {
    int pipes[2];
    char *argv[5];
    if (pipe(pipes) != 0) {
        return -1;
    }
    argv[0] = "sudo";
    argv[1] = "-n";
    argv[2] = "tee";
    argv[3] = (char *)path;
    argv[4] = NULL;
    if (write(pipes[1], text, strlen(text)) < 0) {
        int err = errno;
        close(pipes[0]);
        close(pipes[1]);
        errno = err;
        return -1;
    }
    close(pipes[1]);
    if (sudo_run(argv, pipes[0]) != 0) {
        close(pipes[0]);
        return -1;
    }
    close(pipes[0]);
    return 0;
}

static int write_text(const char *path, const char *text) {
    if (write_fd(path, text) == 0) {
        return 0;
    }
    return write_via_sudo(path, text);
}

static int moved_into(const char *dir) {
    const char *leaf = strrchr(dir, '/');
    char cg[8192];
    char needle[PATH_MAX];
    int fd;
    ssize_t n;
    leaf = leaf ? leaf + 1 : dir;
    if (leaf[0] == 0) {
        return 0;
    }
    snprintf(needle, sizeof needle, "/%s", leaf);
    fd = open("/proc/self/cgroup", O_RDONLY | O_CLOEXEC);
    if (fd < 0) {
        return 0;
    }
    n = read(fd, cg, sizeof cg - 1);
    close(fd);
    if (n < 0) {
        return 0;
    }
    cg[n] = 0;
    {
        const char *hit = cg;
        size_t len = strlen(needle);
        while ((hit = strstr(hit, needle)) != NULL) {
            char next = hit[len];
            if (next == '\0' || next == '\n' || next == '/') {
                return 1;
            }
            hit++;
        }
    }
    return 0;
}

static int write_tid(const char *threads, int tid) {
    char buf[32];
    snprintf(buf, sizeof buf, "%d", tid);
    return write_text(threads, buf);
}

static int move_threads(const char *threads) {
    DIR *dir = opendir("/proc/self/task");
    struct dirent *ent;
    int ok = 1;
    if (!dir) {
        return -1;
    }
    while ((ent = readdir(dir)) != NULL) {
        int tid;
        if (ent->d_name[0] == '.') {
            continue;
        }
        tid = atoi(ent->d_name);
        if (tid <= 0) {
            continue;
        }
        if (write_tid(threads, tid) != 0) {
            ok = 0;
        }
    }
    closedir(dir);
    return ok ? 0 : -1;
}

int ljos_test_enter_cgroup(const char *dir) {
    char procs[PATH_MAX];
    char threads[PATH_MAX];
    char typep[PATH_MAX];
    char pidbuf[32];
    int attempt;
    if (dir == NULL || dir[0] != '/') {
        errno = EINVAL;
        return -1;
    }
    if (mkdir_p(dir) != 0) {
        return -1;
    }
    if (delegate(dir) != 0) {
        /* Root can still write the procs file. Keep going. */
    }
    if (snprintf(procs, sizeof procs, "%s/cgroup.procs", dir) >= (int)sizeof procs ||
        snprintf(threads, sizeof threads, "%s/cgroup.threads", dir) >= (int)sizeof threads ||
        snprintf(typep, sizeof typep, "%s/cgroup.type", dir) >= (int)sizeof typep) {
        errno = ENAMETOOLONG;
        return -1;
    }
    snprintf(pidbuf, sizeof pidbuf, "%d", (int)getpid());
    for (attempt = 0; attempt < 5; attempt++) {
        if (write_text(procs, pidbuf) == 0 && moved_into(dir)) {
            return 0;
        }
        usleep(20000);
    }
    {
        char type[80];
        int fd = open(typep, O_RDONLY | O_CLOEXEC);
        ssize_t n = -1;
        type[0] = 0;
        if (fd >= 0) {
            n = read(fd, type, sizeof type - 1);
            close(fd);
        }
        if (n > 0) {
            type[n] = 0;
        }
        /* A child of a threaded cgroup is domain invalid. cgroup.procs
         * refuses it. Threads can move once the child is threaded too. */
        if (strstr(type, "invalid") != NULL && write_text(typep, "threaded") != 0) {
            errno = EBUSY;
            return -1;
        }
        if (strstr(type, "invalid") != NULL || strstr(type, "threaded") != NULL) {
            if (move_threads(threads) == 0 && moved_into(dir)) {
                return 0;
            }
        }
    }
    for (attempt = 0; attempt < 5; attempt++) {
        if (write_text(procs, pidbuf) == 0 && moved_into(dir)) {
            return 0;
        }
        usleep(20000);
    }
    errno = EBUSY;
    return -1;
}

static int key_is_runner_mark(const char *key) {
    static const char *const prefixes[] = {"CURSOR_", "CLAUDE", "CODEX_", "GROK_",
                                           "GEMINI_CLI", "OPENCODE_", "AMP_"};
    static const char *const exact[] = {
        "CLOUD_AGENT_ID",     "AGENT_TOOL_NAME",    "LJOS_HARNESS_SESSION",
        "LJOS_SESSION_ID",    "LJOS_SHELL_SESSION", "LJOS_PANEL_HARNESS",
        "LJOS_IN_HOOK",       "CLAUDECODE",         "CODEX_SANDBOX",
        "GEMINI_CLI"};
    size_t i;
    size_t n = strlen(key);
    for (i = 0; i < sizeof prefixes / sizeof prefixes[0]; i++) {
        if (starts_ci(key, prefixes[i])) {
            return 1;
        }
    }
    for (i = 0; i < sizeof exact / sizeof exact[0]; i++) {
        if (eq_ci(key, exact[i])) {
            return 1;
        }
    }
    if (strcmp(key, "XDG_SESSION_ID") == 0 || strcmp(key, "BLE_SESSION_ID") == 0) {
        return 0;
    }
    if (n >= 11 && strcmp(key + n - 11, "_SESSION_ID") == 0) {
        return 1;
    }
    if (n >= 10 && strcmp(key + n - 10, "_THREAD_ID") == 0) {
        return 1;
    }
    if (n >= 16 && strcmp(key + n - 16, "_CONVERSATION_ID") == 0) {
        return 1;
    }
    return 0;
}

static void strip_runner_env(void) {
    int progress = 1;
    while (progress) {
        char **env;
        progress = 0;
        for (env = environ; env && *env; env++) {
            char *eq = strchr(*env, '=');
            size_t len;
            char *copy;
            if (!eq || eq == *env) {
                continue;
            }
            len = (size_t)(eq - *env);
            if (len > 256) {
                continue;
            }
            copy = malloc(len + 1);
            if (!copy) {
                continue;
            }
            memcpy(copy, *env, len);
            copy[len] = 0;
            if (key_is_runner_mark(copy)) {
                unsetenv(copy);
                free(copy);
                progress = 1;
                break;
            }
            free(copy);
        }
    }
}

static void leave_foreign_cgroup(void) {
    char text[8192];
    char dir[128];
    int fd;
    ssize_t n;
    strip_runner_env();
    fd = open("/proc/self/cgroup", O_RDONLY | O_CLOEXEC);
    if (fd < 0) {
        return;
    }
    n = read(fd, text, sizeof text - 1);
    close(fd);
    if (n < 0) {
        return;
    }
    text[n] = 0;
    if (!cgroup_is_foreign(text)) {
        return;
    }
    snprintf(dir, sizeof dir, "/sys/fs/cgroup/ljos-session-%u", (unsigned)getuid());
    if (ljos_test_enter_cgroup(dir) == 0) {
        return;
    }
    snprintf(dir, sizeof dir, "/sys/fs/cgroup/ljos-session-%u-%d", (unsigned)getuid(),
             (int)getpid());
    if (ljos_test_enter_cgroup(dir) != 0) {
        fprintf(stderr, "ljos test: could not leave host cgroup %s\n", text);
    }
}

__attribute__((constructor(65535))) void ljos_test_session_cgroup(void) {
    leave_foreign_cgroup();
}

#else

int ljos_test_enter_cgroup(const char *dir) {
    (void)dir;
    return -1;
}

void ljos_test_session_cgroup(void) {}

#endif
