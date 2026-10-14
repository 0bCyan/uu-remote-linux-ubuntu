// Verifies the desktop user's own password with PAM for remote auto-unlock (SimpleAction 15).
// libpam is loaded at runtime, so building needs no PAM headers and hosts without libpam simply
// report auto-unlock as unsupported. The PAM ABI types below are stable (Linux-PAM / OpenPAM).
// Non-root pam_unix checks the caller's own password through its setgid unix_chkpwd helper.
#include <dlfcn.h>
#include <stdlib.h>
#include <string.h>

struct pam_message { int msg_style; const char *msg; };
struct pam_response { char *resp; int resp_retcode; };
struct pam_conv {
    int (*conv)(int, const struct pam_message **, struct pam_response **, void *);
    void *appdata_ptr;
};
typedef struct pam_handle pam_handle_t;

#define PAM_SUCCESS 0
#define PAM_BUF_ERR 5
#define PAM_CONV_ERR 19
#define PAM_AUTH_ERR 7
#define PAM_PROMPT_ECHO_OFF 1
#define PAM_PROMPT_ECHO_ON 2
#define PAM_SILENT 0x8000

typedef int (*start_fn)(const char *, const char *, const struct pam_conv *, pam_handle_t **);
typedef int (*auth_fn)(pam_handle_t *, int);
typedef int (*end_fn)(pam_handle_t *, int);

static void *load_pam(void) {
    void *lib = dlopen("libpam.so.0", RTLD_NOW | RTLD_LOCAL);
    return lib ? lib : dlopen("libpam.so", RTLD_NOW | RTLD_LOCAL);
}

static int conversation(int n, const struct pam_message **msg, struct pam_response **resp, void *data) {
    if (n <= 0) return PAM_CONV_ERR;
    struct pam_response *out = calloc((size_t)n, sizeof *out);
    if (!out) return PAM_BUF_ERR;
    for (int i = 0; i < n; i++) {
        // Answer the password prompt only; informational messages get empty responses.
        if (msg[i]->msg_style == PAM_PROMPT_ECHO_OFF) {
            out[i].resp = strdup((const char *)data);
            if (!out[i].resp) { free(out); return PAM_BUF_ERR; }
        } else if (msg[i]->msg_style == PAM_PROMPT_ECHO_ON) {
            free(out); return PAM_CONV_ERR; // unexpected (e.g. username/OTP): do not guess
        }
    }
    *resp = out;
    return PAM_SUCCESS;
}

int uu_pam_available(void) {
    void *lib = load_pam();
    if (!lib) return 0;
    int ok = dlsym(lib, "pam_start") && dlsym(lib, "pam_authenticate") && dlsym(lib, "pam_end");
    dlclose(lib);
    return ok;
}

// 0 password accepted, 1 rejected, -1 PAM unavailable or failed to run.
int uu_pam_check(const char *service, const char *user, const char *password) {
    void *lib = load_pam();
    if (!lib) return -1;
    start_fn start = (start_fn)dlsym(lib, "pam_start");
    auth_fn authenticate = (auth_fn)dlsym(lib, "pam_authenticate");
    end_fn end = (end_fn)dlsym(lib, "pam_end");
    if (!start || !authenticate || !end) { dlclose(lib); return -1; }
    struct pam_conv conv = { conversation, (void *)password };
    pam_handle_t *handle = NULL;
    int rc = start(service, user, &conv, &handle);
    if (rc != PAM_SUCCESS) { dlclose(lib); return -1; }
    rc = authenticate(handle, PAM_SILENT);
    end(handle, rc);
    dlclose(lib);
    if (rc == PAM_SUCCESS) return 0;
    return rc == PAM_AUTH_ERR ? 1 : -1;
}
