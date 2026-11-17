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
#include <signal.h>
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
    "cursor",   "grok",        "example-host",  "windsurf",  "vscode",    "claude-desktop",
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
    static const char *const prefixes[] = {"CURSOR_", "CLAUDE", "CODEX_", "SEAT_",
                                           "GEMINI_CLI", "OPENCODE_", "AMP_"};
    static const char *const exact[] = {
        "CLOUD_AGENT_ID",     "AGENT_TOOL_NAME",    "LJOS_HARNESS_SESSION",
        "LJOS_SESSION_ID",    "LJOS_MEM_PORT",      "LJOS_SHELL_SESSION",
        "LJOS_PANEL_HARNESS", "LJOS_IN_HOOK",       "CLAUDECODE",
        "CODEX_SANDBOX",      "GEMINI_CLI"};
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

/* 8761 is 0x2239. A test may detach a packsetd there. At exit this
 * process stops only a writer it started: the start time is later than
 * this process, the path names this pid, or the parent chain reaches
 * this process, and the writer is not serving another HOME. A writer
 * that was already listening is left alone. SIGTERM, then SIGKILL
 * after two seconds. */
#define PACK_PORT_MARK ":2239"
#define KEPT_MAX 128

static pid_t kept_pid[KEPT_MAX];
static int kept_count;
static unsigned long long self_start;

static int already_kept(pid_t pid) {
    int i;
    for (i = 0; i < kept_count; i++) {
        if (kept_pid[i] == pid) {
            return 1;
        }
    }
    return 0;
}

static int field_copy(const char *start, char *out, size_t cap) {
    size_t n = 0;
    while (start[n] && start[n] != ' ' && n + 1 < cap) {
        out[n] = start[n];
        n++;
    }
    out[n] = 0;
    return n > 0;
}

/* inode of a listening socket on the default port, or 0. */
static unsigned long inode_on_default(const char *line) {
    const char *field[12];
    char local[128];
    char state[16];
    char inode[32];
    int n = 0;
    const char *p = line;
    while (*p && n < 12) {
        while (*p == ' ') {
            p++;
        }
        if (!*p) {
            break;
        }
        field[n++] = p;
        while (*p && *p != ' ') {
            p++;
        }
    }
    if (n < 10) {
        return 0;
    }
    if (!field_copy(field[1], local, sizeof local)) {
        return 0;
    }
    if (!field_copy(field[3], state, sizeof state)) {
        return 0;
    }
    if (strcmp(state, "0A") != 0) {
        return 0;
    }
    if (strlen(local) < strlen(PACK_PORT_MARK) ||
        strcmp(local + strlen(local) - strlen(PACK_PORT_MARK), PACK_PORT_MARK) != 0) {
        return 0;
    }
    if (!field_copy(field[9], inode, sizeof inode)) {
        return 0;
    }
    return strtoul(inode, 0, 10);
}

static int tables_have_inode(unsigned long inode) {
    static const char *const tables[] = {"/proc/net/tcp", "/proc/net/tcp6"};
    size_t t;
    for (t = 0; t < 2; t++) {
        FILE *file = fopen(tables[t], "r");
        char line[512];
        if (!file) {
            continue;
        }
        if (!fgets(line, sizeof line, file)) {
            fclose(file);
            continue;
        }
        while (fgets(line, sizeof line, file)) {
            if (inode_on_default(line) == inode) {
                fclose(file);
                return 1;
            }
        }
        fclose(file);
    }
    return 0;
}

static int packsetd_on_default(pid_t pid) {
    char path[64];
    char comm[64];
    char link[64];
    DIR *fds;
    struct dirent *ent;
    FILE *file;
    int found = 0;
    snprintf(path, sizeof path, "/proc/%d/comm", (int)pid);
    file = fopen(path, "r");
    if (!file) {
        return 0;
    }
    if (!fgets(comm, sizeof comm, file)) {
        fclose(file);
        return 0;
    }
    fclose(file);
    if (strcmp(comm, "packsetd\n") != 0) {
        return 0;
    }
    snprintf(path, sizeof path, "/proc/%d/fd", (int)pid);
    fds = opendir(path);
    if (!fds) {
        return 0;
    }
    while ((ent = readdir(fds)) != 0) {
        ssize_t n;
        unsigned long inode = 0;
        if (ent->d_name[0] == '.') {
            continue;
        }
        snprintf(path, sizeof path, "/proc/%d/fd/%s", (int)pid, ent->d_name);
        n = readlink(path, link, sizeof link - 1);
        if (n < 0) {
            continue;
        }
        link[n] = 0;
        if (sscanf(link, "socket:[%lu]", &inode) == 1 && inode != 0 &&
            tables_have_inode(inode)) {
            found = 1;
            break;
        }
    }
    closedir(fds);
    return found;
}

static void remember_default_writers(void) {
    DIR *proc = opendir("/proc");
    struct dirent *ent;
    if (!proc) {
        return;
    }
    while ((ent = readdir(proc)) != 0 && kept_count < KEPT_MAX) {
        char *end = 0;
        long pid = strtol(ent->d_name, &end, 10);
        if (!end || *end || pid <= 0) {
            continue;
        }
        if (packsetd_on_default((pid_t)pid)) {
            kept_pid[kept_count++] = (pid_t)pid;
        }
    }
    closedir(proc);
}

static unsigned long long start_ticks(pid_t pid) {
    char path[64];
    FILE *file;
    char buf[1024];
    char *end;
    int field;
    snprintf(path, sizeof path, "/proc/%d/stat", (int)pid);
    file = fopen(path, "r");
    if (!file) {
        return 0;
    }
    if (!fgets(buf, sizeof buf, file)) {
        fclose(file);
        return 0;
    }
    fclose(file);
    end = strrchr(buf, ')');
    if (!end) {
        return 0;
    }
    end++;
    /* state is field 3. starttime is field 22, so skip 3 through 21. */
    for (field = 3; field < 22; field++) {
        while (*end == ' ') {
            end++;
        }
        if (!*end) {
            return 0;
        }
        while (*end && *end != ' ') {
            end++;
        }
    }
    while (*end == ' ') {
        end++;
    }
    return strtoull(end, 0, 10);
}

static pid_t parent_of(pid_t pid) {
    char path[64];
    FILE *file;
    char line[256];
    snprintf(path, sizeof path, "/proc/%d/status", (int)pid);
    file = fopen(path, "r");
    if (!file) {
        return 0;
    }
    while (fgets(line, sizeof line, file)) {
        if (strncmp(line, "PPid:", 5) == 0) {
            fclose(file);
            return (pid_t)atoi(line + 5);
        }
    }
    fclose(file);
    return 0;
}

static int read_link_path(const char *path, char *out, size_t cap) {
    ssize_t n = readlink(path, out, cap - 1);
    if (n < 0) {
        return 0;
    }
    out[n] = 0;
    return 1;
}

static int read_proc_home(pid_t pid, char *out, size_t cap) {
    char path[64];
    FILE *file;
    char buf[8192];
    size_t n;
    size_t i = 0;
    snprintf(path, sizeof path, "/proc/%d/environ", (int)pid);
    file = fopen(path, "r");
    if (!file) {
        return 0;
    }
    n = fread(buf, 1, sizeof buf, file);
    fclose(file);
    while (i < n) {
        if (strncmp(buf + i, "HOME=", 5) == 0) {
            size_t len = 0;
            const char *value = buf + i + 5;
            while (i + 5 + len < n && value[len] != 0 && len + 1 < cap) {
                out[len] = value[len];
                len++;
            }
            out[len] = 0;
            while (len > 1 && out[len - 1] == '/') {
                out[--len] = 0;
            }
            return 1;
        }
        while (i < n && buf[i] != 0) {
            i++;
        }
        if (i < n) {
            i++;
        }
    }
    return 0;
}

static int read_proc_text(const char *path, char *out, size_t cap) {
    FILE *file = fopen(path, "r");
    size_t n;
    size_t i;
    if (!file) {
        return 0;
    }
    n = fread(out, 1, cap - 1, file);
    fclose(file);
    for (i = 0; i < n; i++) {
        if (out[i] == 0) {
            out[i] = ' ';
        }
    }
    out[n] = 0;
    return n > 0;
}

static int path_has_our_pid(const char *text) {
    char dash[32];
    char slash[32];
    if (!text || !text[0]) {
        return 0;
    }
    snprintf(dash, sizeof dash, "-%d-", (int)getpid());
    snprintf(slash, sizeof slash, "-%d/", (int)getpid());
    return strstr(text, dash) != 0 || strstr(text, slash) != 0;
}

static int ancestor_is_us(pid_t pid) {
    int step;
    for (step = 0; step < 16; step++) {
        pid_t parent = parent_of(pid);
        if (parent <= 1) {
            return 0;
        }
        if (parent == getpid()) {
            return 1;
        }
        pid = parent;
    }
    return 0;
}

/* A HOME that is not this process, and not a directory whose name carries
 * this pid, is another seat's writer. */
static int serves_another_home(pid_t pid) {
    char home[4096];
    char ours[4096];
    const char *env = getenv("HOME");
    size_t n;
    if (!read_proc_home(pid, home, sizeof home) || home[0] == 0) {
        return 0;
    }
    if (!env) {
        env = "";
    }
    n = strlen(env);
    while (n > 1 && env[n - 1] == '/') {
        n--;
    }
    if (n >= sizeof ours) {
        n = sizeof ours - 1;
    }
    memcpy(ours, env, n);
    ours[n] = 0;
    if (strcmp(home, ours) == 0) {
        return 0;
    }
    if (path_has_our_pid(home)) {
        return 0;
    }
    return 1;
}

static int started_by_us(pid_t pid) {
    char cwd_path[64];
    char cmd_path[64];
    char cwd[4096];
    char cmd[8192];
    char home[4096];
    if (ancestor_is_us(pid)) {
        return 1;
    }
    snprintf(cwd_path, sizeof cwd_path, "/proc/%d/cwd", (int)pid);
    if (read_link_path(cwd_path, cwd, sizeof cwd) && path_has_our_pid(cwd)) {
        return 1;
    }
    snprintf(cmd_path, sizeof cmd_path, "/proc/%d/cmdline", (int)pid);
    if (read_proc_text(cmd_path, cmd, sizeof cmd) && path_has_our_pid(cmd)) {
        return 1;
    }
    if (read_proc_home(pid, home, sizeof home) && path_has_our_pid(home)) {
        return 1;
    }
    return 0;
}

static int ours_to_stop(pid_t pid) {
    unsigned long long began;
    if (self_start == 0) {
        return 0;
    }
    began = start_ticks(pid);
    if (began == 0 || began <= self_start) {
        return 0;
    }
    if (serves_another_home(pid)) {
        return 0;
    }
    return started_by_us(pid);
}

static int alive_pid(pid_t pid) {
    char path[64];
    FILE *file;
    char buf[1024];
    char *end;
    snprintf(path, sizeof path, "/proc/%d/stat", (int)pid);
    file = fopen(path, "r");
    if (file) {
        if (fgets(buf, sizeof buf, file)) {
            end = strrchr(buf, ')');
            if (end && end[1] == ' ' && end[2] == 'Z') {
                fclose(file);
                return 0;
            }
        }
        fclose(file);
    }
    if (kill(pid, 0) == 0) {
        return 1;
    }
    return errno != ESRCH;
}

static void stop_politely(pid_t pid) {
    int i;
    kill(pid, SIGTERM);
    for (i = 0; i < 40; i++) {
        if (!alive_pid(pid)) {
            return;
        }
        usleep(50000);
    }
    kill(pid, SIGKILL);
}

static void reap_default_writers(void) {
    DIR *proc = opendir("/proc");
    struct dirent *ent;
    if (!proc) {
        return;
    }
    while ((ent = readdir(proc)) != 0) {
        char *end = 0;
        long pid = strtol(ent->d_name, &end, 10);
        if (!end || *end || pid <= 1) {
            continue;
        }
        if (already_kept((pid_t)pid)) {
            continue;
        }
        if (!packsetd_on_default((pid_t)pid)) {
            continue;
        }
        if (!ours_to_stop((pid_t)pid)) {
            continue;
        }
        stop_politely((pid_t)pid);
    }
    closedir(proc);
}

__attribute__((constructor(65535))) void ljos_test_session_cgroup(void) {
    leave_foreign_cgroup();
    self_start = start_ticks(getpid());
    remember_default_writers();
    atexit(reap_default_writers);
}

#else

int ljos_test_enter_cgroup(const char *dir) {
    (void)dir;
    return -1;
}

void ljos_test_session_cgroup(void) {}

#endif
