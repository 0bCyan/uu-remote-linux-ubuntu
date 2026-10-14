/* Text-only X11 selection bridge. Each worker owns its own Display connection.
 * Existing selection contents are not requested when the bridge starts.
 * XFixes reports subsequent copies, including copies by the same owner. */
#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <X11/extensions/Xfixes.h>
#include <stdlib.h>
#include <string.h>

#define CLIP_LIMIT (64 * 1024)
typedef struct {
    Display *display;
    Window window;
    Atom selection, utf8, targets, text, property;
    int fixes_event;
    Time request_time;
    unsigned char *owned;
    size_t owned_length;
    int owns_selection;
} NativeClipboard;

void *clipboard_open(const char *display_name) {
    NativeClipboard *c = calloc(1, sizeof(*c));
    if (!c) return NULL;
    c->display = XOpenDisplay(display_name);
    if (!c->display) { free(c); return NULL; }
    int error;
    if (!XFixesQueryExtension(c->display, &c->fixes_event, &error)) {
        XCloseDisplay(c->display); free(c); return NULL;
    }
    c->window = XCreateSimpleWindow(c->display, DefaultRootWindow(c->display),
                                    0, 0, 1, 1, 0, 0, 0);
    c->selection = XInternAtom(c->display, "CLIPBOARD", False);
    c->utf8 = XInternAtom(c->display, "UTF8_STRING", False);
    c->targets = XInternAtom(c->display, "TARGETS", False);
    c->text = XInternAtom(c->display, "TEXT", False);
    c->property = XInternAtom(c->display, "UU_NATIVE_CLIPBOARD", False);
    XFixesSelectSelectionInput(c->display, c->window, c->selection,
        XFixesSetSelectionOwnerNotifyMask | XFixesSelectionWindowDestroyNotifyMask |
        XFixesSelectionClientCloseNotifyMask);
    XFlush(c->display);
    return c;
}

int clipboard_set(void *handle, const unsigned char *text, size_t length) {
    NativeClipboard *c = handle;
    if (length > CLIP_LIMIT) return -1;
    unsigned char *copy = malloc(length + 1);
    if (!copy) return -1;
    memcpy(copy, text, length); copy[length] = 0;
    XSetSelectionOwner(c->display, c->selection, c->window, CurrentTime);
    if (XGetSelectionOwner(c->display, c->selection) != c->window) {
        free(copy); return -1;
    }
    free(c->owned); c->owned = copy; c->owned_length = length;
    c->owns_selection = 1;
    XFlush(c->display);
    return 0;
}

static void answer_request(NativeClipboard *c, XSelectionRequestEvent *r) {
    XEvent reply; memset(&reply, 0, sizeof(reply));
    reply.xselection.type = SelectionNotify;
    reply.xselection.display = r->display;
    reply.xselection.requestor = r->requestor;
    reply.xselection.selection = r->selection;
    reply.xselection.target = r->target;
    reply.xselection.time = r->time;
    reply.xselection.property = None;
    Atom property = r->property == None ? r->target : r->property;
    if (r->selection == c->selection && c->owns_selection) {
        if (r->target == c->targets) {
            Atom targets[] = { c->targets, c->utf8, c->text };
            XChangeProperty(c->display, r->requestor, property, XA_ATOM, 32,
                            PropModeReplace, (unsigned char *)targets, 3);
            reply.xselection.property = property;
        } else if (r->target == c->utf8 || r->target == c->text) {
            XChangeProperty(c->display, r->requestor, property, c->utf8, 8,
                            PropModeReplace, c->owned, c->owned_length);
            reply.xselection.property = property;
        }
    }
    XSendEvent(c->display, r->requestor, False, 0, &reply);
    XFlush(c->display);
}

/* -2: no text update, -1: unsupported/oversized conversion, >=0: byte length.
 * INCR and non-UTF8 content are intentionally refused, never guessed as text. */
int clipboard_poll(void *handle, unsigned char *out, size_t capacity) {
    NativeClipboard *c = handle;
    for (int count = 0; count < 64 && XPending(c->display); ++count) {
        XEvent event; XNextEvent(c->display, &event);
        if (event.type == SelectionRequest) {
            answer_request(c, &event.xselectionrequest);
        } else if (event.type == SelectionClear) {
            c->owns_selection = 0;
        } else if (event.type == c->fixes_event + XFixesSelectionNotify) {
            XFixesSelectionNotifyEvent *change = (void *)&event;
            if (change->selection != c->selection || change->owner == c->window ||
                change->owner == None) continue;
            c->request_time = change->selection_timestamp;
            XDeleteProperty(c->display, c->window, c->property);
            XConvertSelection(c->display, c->selection, c->utf8, c->property,
                              c->window, c->request_time);
            XFlush(c->display);
        } else if (event.type == SelectionNotify) {
            XSelectionEvent *n = &event.xselection;
            if (n->selection != c->selection || n->time != c->request_time ||
                n->property == None || c->owns_selection) continue;
            Atom actual; int format; unsigned long length, remaining;
            unsigned char *data = NULL;
            int status = XGetWindowProperty(c->display, c->window, c->property,
                0, (CLIP_LIMIT + 3) / 4, True, AnyPropertyType,
                &actual, &format, &length, &remaining, &data);
            int result = -1;
            if (status == Success && actual == c->utf8 && format == 8 &&
                remaining == 0 && length <= capacity) {
                if (length) memcpy(out, data, length);
                result = (int)length;
            }
            if (data) XFree(data);
            XDeleteProperty(c->display, c->window, c->property);
            return result;
        }
    }
    return -2;
}

void clipboard_close(void *handle) {
    NativeClipboard *c = handle;
    XDestroyWindow(c->display, c->window);
    XCloseDisplay(c->display);
    free(c->owned); free(c);
}
