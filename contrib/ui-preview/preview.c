/* Offline GTK regression harness. No host, network, credentials or tray is started. */
#include "../../c_engine/assist_window.c"
#include <assert.h>

static void settle(void) {
    for (int i = 0; i < 35; i++) {
        while (gtk_events_pending()) gtk_main_iteration();
        g_usleep(10000);
    }
}
static void state(int allowed, int verify) {
    State *s = g_new0(State, 1);
    s->id = g_strdup("123 456 789"); s->code = g_strdup("a1b2c3d4");
    s->allowed = allowed; s->verify = verify; s->refresh = 0; s->has_custom = 1;
    apply_state(s); settle();
}
static void save(const char *dir, const char *name) {
    settle();
    GdkWindow *w = gtk_widget_get_window(window);
    GdkPixbuf *p = gdk_pixbuf_get_from_window(w, 0, 0, gdk_window_get_width(w), gdk_window_get_height(w));
    assert(p);
    char *path = g_build_filename(dir, name, NULL);
    assert(gdk_pixbuf_save(p, path, "png", NULL, NULL));
    g_object_unref(p); g_free(path);
}
static void assert_label_fits(GtkWidget *w) {
    int width, height;
    pango_layout_get_pixel_size(gtk_label_get_layout(GTK_LABEL(w)), &width, &height);
    assert(width <= gtk_widget_get_allocated_width(w));
    assert(height <= gtk_widget_get_allocated_height(w));
}
int main(int argc, char **argv) {
    assert(argc == 2); assert(gtk_init_check(NULL, NULL));
    g_object_set(gtk_settings_get_default(), "gtk-theme-name", "Adwaita", "gtk-application-prefer-dark-theme", FALSE, NULL);
    GtkCssProvider *provider = gtk_css_provider_new(); GError *error = NULL;
    assert(gtk_css_provider_load_from_data(provider, CSS, -1, &error));
    assert(!error); g_object_unref(provider);
    show_account = 1; build_window(); settle();
    state(1, 0); apply_user(g_strdup("示例账号 · 仅供预览"));
    save(argv[1], "main.png");
    assert_label_fits(id_label); assert_label_fits(code_label);
    assert_label_fits(assist_status);
    /* Exercise the real UI callbacks and FIFO using fake values only. */
    gtk_button_clicked(GTK_BUTTON(eye_button));
    assert(strcmp(gtk_label_get_text(GTK_LABEL(code_label)), "********") == 0);
    gtk_button_clicked(GTK_BUTTON(eye_button));
    assert(strcmp(gtk_label_get_text(GTK_LABEL(code_label)), "a1b2c3d4") == 0);
    gtk_button_clicked(GTK_BUTTON(share_button));
    char *clip = gtk_clipboard_wait_for_text(gtk_clipboard_get(GDK_SELECTION_CLIPBOARD));
    assert(clip && strstr(clip, "123 456 789") && strstr(clip, "a1b2c3d4")); g_free(clip);
    on_copy_id(NULL, NULL);
    clip = gtk_clipboard_wait_for_text(gtk_clipboard_get(GDK_SELECTION_CLIPBOARD));
    assert(clip && strcmp(clip, "123456789") == 0); g_free(clip);
    gtk_clipboard_clear(gtk_clipboard_get(GDK_SELECTION_CLIPBOARD));
    char arg[40];
    assert(uu_assist_ui_take_command(arg, sizeof arg) == CMD_NONE);
    gtk_switch_set_active(GTK_SWITCH(allow_switch), FALSE);
    assert(uu_assist_ui_take_command(arg, sizeof arg) == CMD_DENY);
    state(0, 0); save(argv[1], "disabled.png");
    state(1, 0);
    gtk_expander_set_expanded(GTK_EXPANDER(settings_expander), TRUE);
    gtk_combo_box_set_active(GTK_COMBO_BOX(verify_combo), VERIFY_BOTH);
    assert(uu_assist_ui_take_command(arg, sizeof arg) == CMD_VERIFY && strcmp(arg, "2") == 0);
    gtk_check_menu_item_set_active(GTK_CHECK_MENU_ITEM(refresh_items[2]), TRUE);
    assert(uu_assist_ui_take_command(arg, sizeof arg) == CMD_REFRESH && strcmp(arg, "2") == 0);
    on_refresh_now(NULL, NULL);
    assert(uu_assist_ui_take_command(arg, sizeof arg) == CMD_RESET);
    state(1, VERIFY_BOTH); gtk_window_resize(GTK_WINDOW(window), 800, 940); save(argv[1], "settings.png");
    gtk_expander_set_expanded(GTK_EXPANDER(settings_expander), FALSE);
    gtk_stack_set_visible_child_name(GTK_STACK(account_stack), "qr");
    save(argv[1], "login-loading.png");
    /* A deliberately non-scannable fixture exercises the RGBA/QR stack without a credential. */
    cairo_surface_t *surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 180, 180);
    cairo_t *cr = cairo_create(surface);
    cairo_set_source_rgb(cr, 1, 1, 1); cairo_paint(cr);
    cairo_set_source_rgb(cr, .3, .4, .6); cairo_set_font_size(cr, 16);
    cairo_move_to(cr, 43, 84); cairo_show_text(cr, "PREVIEW");
    cairo_set_font_size(cr, 11); cairo_move_to(cr, 34, 106); cairo_show_text(cr, "No login data encoded");
    GdkPixbuf *fixture = gdk_pixbuf_get_from_surface(surface, 0, 0, 180, 180);
    QrUpdate *qr = g_new0(QrUpdate, 1); qr->edge = 180;
    qr->rgba = g_memdup2(gdk_pixbuf_get_pixels(fixture), 180 * 180 * 4);
    apply_qr(qr); g_object_unref(fixture); cairo_destroy(cr); cairo_surface_destroy(surface);
    save(argv[1], "login-fixture.png");
    StatusUpdate *u = g_new0(StatusUpdate, 1);
    u->text = g_strdup("二维码已过期，请刷新后重试。"); u->kind = 2;
    apply_login_status(u); apply_qr_refresh(GINT_TO_POINTER(1));
    save(argv[1], "login-error.png");
    gtk_button_clicked(GTK_BUTTON(qr_refresh));
    assert(uu_login_ui_take_refresh() == 1);
    assert(uu_login_ui_take_refresh() == 0);
    assert(!gtk_widget_get_visible(qr_refresh));
    show_assist = 0; apply_sections(NULL);
    save(argv[1], "login-only.png");
    show_assist = 1; apply_sections(NULL); apply_user(g_strdup("很长的示例账号名称用于检查界面文本的省略显示"));
    gtk_window_resize(GTK_WINDOW(window), 740, 520);
    state(1, 0); save(argv[1], "compact.png");
    assert_label_fits(id_label); assert_label_fits(code_label);
    int normal_width, normal_height;
    pango_layout_get_pixel_size(gtk_label_get_layout(GTK_LABEL(id_label)), &normal_width, &normal_height);
    g_object_set(gtk_settings_get_default(), "gtk-xft-dpi", 144 * 1024, NULL);
    save(argv[1], "large-text.png");
    assert_label_fits(id_label); assert_label_fits(code_label);
    int large_width, large_height;
    pango_layout_get_pixel_size(gtk_label_get_layout(GTK_LABEL(id_label)), &large_width, &large_height);
    assert(large_width > normal_width * 1.3);
    assert(valid_custom("preview123")); assert(!valid_custom("12345678")); assert(!valid_custom("abcdefgh"));
    g_print("PASS: CSS, credential layout, copy, visibility, switch, verification, refresh FIFO, QR retry and large text.\n");
    close_hides = 1;
    assert(on_delete(window, NULL, NULL)); assert(!gtk_widget_get_visible(window));
    present_window(); assert(gtk_widget_get_visible(window));
    assert(uu_assist_ui_take_command(arg, sizeof arg) == CMD_NONE);
    /* The harness iterates GTK without entering gtk_main(). */
    g_signal_handlers_disconnect_by_func(window, G_CALLBACK(on_destroy), NULL);
    gtk_widget_destroy(window);
    return 0;
}
