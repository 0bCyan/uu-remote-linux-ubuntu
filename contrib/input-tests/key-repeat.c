/* Isolated Xvfb only: edit generated fixture text, never the physical desktop. */
#include <gtk/gtk.h>
#include <gdk/gdkx.h>
#include <X11/keysym.h>
#include "../../c_engine/x11_engine.c"
#include <assert.h>
static GtkWidget *entry;
static int ctrl_releases;
static void settle(int ms) {
    for (int i = 0; i < ms / 5; i++) {
        while (gtk_events_pending()) gtk_main_iteration();
        g_usleep(5000);
    }
}
static void reset(void) {
    gtk_entry_set_text(GTK_ENTRY(entry), "abcdefghijklmnopqrst");
    gtk_editable_set_position(GTK_EDITABLE(entry), -1); settle(30);
}
static int length(void) { return gtk_entry_get_text_length(GTK_ENTRY(entry)); }
static gboolean released(GtkWidget *w, GdkEventKey *e, gpointer data) {
    (void)w; (void)data;
    if (e->keyval == GDK_KEY_Control_L) ctrl_releases++;
    return FALSE;
}
int main(void) {
    const char *display = getenv("DISPLAY");
    assert(display && strcmp(display, ":98") == 0);
    assert(gtk_init_check(NULL, NULL)); assert(x11_init(display) == 0);
    GtkWidget *w = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    entry = gtk_entry_new(); gtk_container_add(GTK_CONTAINER(w), entry);
    g_signal_connect(entry, "key-release-event", G_CALLBACK(released), NULL);
    gtk_widget_show_all(w); gtk_widget_grab_focus(entry); settle(100);
    XSetInputFocus(g_display, GDK_WINDOW_XID(gtk_widget_get_window(w)), RevertToParent, CurrentTime);
    XSync(g_display, False); settle(50);
    XKeyboardState initial; XGetKeyboardControl(g_display, &initial);
    reset();
    for (int i = 0; i < 5; i++) { assert(x11_key_symbol(XK_BackSpace, 1) == 0); settle(35); }
    assert(x11_key_symbol(XK_BackSpace, 0) == 0); settle(40);
    printf("explicit_repeated_down: expected_deletions=5 observed=%d\n",20-length()); fflush(stdout);
    assert(length() == 15);
    reset();
    KeyCode backspace = XKeysymToKeycode(g_display, XK_BackSpace);
    for (int i = 0; i < 7; i++) { assert(x11_key_event(backspace, 1) == 0); settle(25); }
    assert(x11_key_event(backspace, 0) == 0); settle(40); assert(length() == 13);
    gtk_entry_set_text(GTK_ENTRY(entry), "");
    for (int i = 0; i < 4; i++) { x11_key_symbol(XK_x, 1); settle(20); }
    x11_key_symbol(XK_x, 0); settle(30); assert(length() == 4);
    reset();
    for (int i = 0; i < 4; i++) { x11_key_symbol(XK_Left, 1); settle(20); }
    x11_key_symbol(XK_Left, 0); settle(30);
    assert(gtk_editable_get_position(GTK_EDITABLE(entry)) == 16);
    reset(); x11_key_symbol(XK_BackSpace, 1); settle(800);
    assert(length() == 19); /* Delayed release must not invent extra deletions. */
    x11_key_symbol(XK_BackSpace, 0); settle(100); assert(length() == 19);
    reset();
    ctrl_releases = 0;
    x11_key_symbol(XK_Control_L, 1); settle(20);
    x11_key_symbol(XK_Control_L, 1); settle(20);
    assert(ctrl_releases == 0);
    x11_key_symbol(XK_a, 1); x11_key_symbol(XK_a, 0);
    x11_key_symbol(XK_Control_L, 0); settle(30);
    assert(ctrl_releases == 1);
    x11_key_symbol(XK_BackSpace, 1); x11_key_symbol(XK_BackSpace, 0); settle(30);
    assert(length() == 0);
    reset(); x11_key_symbol(XK_BackSpace, 1); settle(20); x11_release_all(); settle(20);
    XKeyboardState final; XGetKeyboardControl(g_display, &final);
    assert(initial.global_auto_repeat == final.global_auto_repeat);
    char keys[32]; XQueryKeymap(g_display, keys);
    assert(!(keys[backspace >> 3] & (1u << (backspace & 7))));
    assert(g_injected_count == 0);
    gtk_widget_destroy(w);
    printf("PASS: symbolic/raw repeats, delayed release, modifier stability, shortcut and session cleanup.\n");
}
