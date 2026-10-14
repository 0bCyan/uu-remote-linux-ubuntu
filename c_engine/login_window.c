// GTK3 QR login window for `uuremote-server login`. The window runs its own GTK main loop on a
// dedicated thread; the Rust login flow pushes QR images and status text through g_idle_add.
// Layout follows the official login window (QR on the left), but uses no NetEase assets: the
// mark is drawn with cairo and the right column shows this host's hardware summary instead of SMS.
#include <gtk/gtk.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

static const char *CSS =
    "window { background-image: linear-gradient(165deg, #d6e8ff 0%, #eaf3ff 40%, #ffffff 100%); }\n"
    "* { font-family: 'Noto Sans CJK SC', 'Noto Sans SC', sans-serif; }\n"
    ".title { font-size: 26px; font-weight: 800; color: #111111; }\n"
    ".subtitle { font-size: 13px; color: #6b7280; }\n"
    ".heading { font-size: 19px; font-weight: 700; color: #1d1d1f; }\n"
    ".card { background-color: #ffffff; border-radius: 10px; padding: 14px; }\n"
    ".caption { font-size: 14px; color: #555555; }\n"
    ".status { font-size: 13px; color: #2f6bff; }\n"
    ".status.ok { color: #16a34a; }\n"
    ".status.error { color: #dc2626; }\n"
    ".key { font-size: 13px; color: #8a8f98; }\n"
    ".value { font-size: 13px; color: #222222; }\n"
    ".note { font-size: 12px; color: #6b7280; }\n"
    ".done-mark { font-size: 72px; color: #16a34a; }\n"
    ".done-text { font-size: 17px; font-weight: 700; color: #1d1d1f; }\n"
    ".footer { font-size: 12px; color: #9ca3af; }\n"
    "button.primary { background-image: none; background-color: #2f6bff; color: #ffffff; border: none;"
    " border-radius: 6px; padding: 6px 28px; font-size: 15px; }\n"
    "button.primary:hover { background-color: #1f5ae6; }\n"
    "separator { background-color: #dfe5ee; min-width: 1px; }\n";

typedef struct { unsigned char *rgba; int edge; } QrUpdate;
typedef struct { char *text; int kind; } StatusUpdate;

static atomic_int closed;
static pthread_t ui_thread;
static int ui_started;
static pthread_mutex_t init_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t init_cond = PTHREAD_COND_INITIALIZER;
static int init_state; // 0 pending, 1 ok, -1 failed
static char *info[4];
static GtkWidget *window, *stack, *qr_image, *status_label, *done_label, *done_button;

static gboolean draw_mark(GtkWidget *w, cairo_t *cr, gpointer data) {
    (void)w; (void)data;
    cairo_pattern_t *g = cairo_pattern_create_linear(0, 0, 44, 44);
    cairo_pattern_add_color_stop_rgb(g, 0, 0.13, 0.83, 0.93);
    cairo_pattern_add_color_stop_rgb(g, 1, 0.18, 0.42, 1.0);
    double r = 11, s = 44;
    cairo_new_sub_path(cr);
    cairo_arc(cr, s - r, r, r, -G_PI / 2, 0);
    cairo_arc(cr, s - r, s - r, r, 0, G_PI / 2);
    cairo_arc(cr, r, s - r, r, G_PI / 2, G_PI);
    cairo_arc(cr, r, r, r, G_PI, 3 * G_PI / 2);
    cairo_close_path(cr);
    cairo_set_source(cr, g);
    cairo_fill(cr);
    cairo_pattern_destroy(g);
    cairo_set_source_rgb(cr, 1, 1, 1);
    cairo_select_font_face(cr, "sans-serif", CAIRO_FONT_SLANT_NORMAL, CAIRO_FONT_WEIGHT_BOLD);
    cairo_set_font_size(cr, 19);
    cairo_text_extents_t e;
    cairo_text_extents(cr, "UU", &e);
    cairo_move_to(cr, (s - e.width) / 2 - e.x_bearing, (s - e.height) / 2 - e.y_bearing);
    cairo_show_text(cr, "UU");
    return TRUE;
}

static GtkWidget *label(const char *text, const char *cls) {
    GtkWidget *l = gtk_label_new(text);
    gtk_style_context_add_class(gtk_widget_get_style_context(l), cls);
    return l;
}

static void info_row(GtkGrid *grid, int row, const char *key, const char *value) {
    GtkWidget *k = label(key, "key"), *v = label(value && *value ? value : "-", "value");
    gtk_widget_set_halign(k, GTK_ALIGN_START);
    gtk_widget_set_valign(k, GTK_ALIGN_START);
    gtk_widget_set_halign(v, GTK_ALIGN_START);
    gtk_label_set_xalign(GTK_LABEL(v), 0);
    gtk_label_set_line_wrap(GTK_LABEL(v), TRUE);
    gtk_label_set_max_width_chars(GTK_LABEL(v), 26);
    gtk_grid_attach(grid, k, 0, row, 1, 1);
    gtk_grid_attach(grid, v, 1, row, 1, 1);
}

static void on_destroy(GtkWidget *w, gpointer data) {
    (void)w; (void)data;
    atomic_store(&closed, 1);
    window = NULL;
    gtk_main_quit();
}

static void on_done_clicked(GtkButton *b, gpointer data) {
    (void)b; (void)data;
    if (window) gtk_widget_destroy(window);
}

static void build_window(void) {
    GtkCssProvider *css = gtk_css_provider_new();
    gtk_css_provider_load_from_data(css, CSS, -1, NULL);
    gtk_style_context_add_provider_for_screen(gdk_screen_get_default(), GTK_STYLE_PROVIDER(css),
                                              GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
    g_object_unref(css);

    window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), "UU 远程 Linux 主机 - 登录");
    gtk_window_set_default_size(GTK_WINDOW(window), 720, 520);
    gtk_window_set_resizable(GTK_WINDOW(window), FALSE);
    gtk_window_set_position(GTK_WINDOW(window), GTK_WIN_POS_CENTER);
    g_signal_connect(window, "destroy", G_CALLBACK(on_destroy), NULL);

    GtkWidget *root = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    gtk_container_set_border_width(GTK_CONTAINER(root), 24);
    gtk_container_add(GTK_CONTAINER(window), root);

    GtkWidget *header = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 12);
    gtk_widget_set_halign(header, GTK_ALIGN_CENTER);
    GtkWidget *mark = gtk_drawing_area_new();
    gtk_widget_set_size_request(mark, 44, 44);
    gtk_widget_set_valign(mark, GTK_ALIGN_CENTER);
    g_signal_connect(mark, "draw", G_CALLBACK(draw_mark), NULL);
    GtkWidget *titles = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    GtkWidget *title = label("UU 远程 · Linux 主机", "title");
    gtk_widget_set_halign(title, GTK_ALIGN_START);
    GtkWidget *subtitle = label("登录后，本机会出现在你的 UU 远程设备列表中", "subtitle");
    gtk_widget_set_halign(subtitle, GTK_ALIGN_START);
    gtk_box_pack_start(GTK_BOX(titles), title, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(titles), subtitle, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(header), mark, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(header), titles, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(root), header, FALSE, FALSE, 4);

    GtkWidget *columns = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
    gtk_box_set_homogeneous(GTK_BOX(columns), FALSE);
    gtk_widget_set_margin_top(columns, 22);
    gtk_box_pack_start(GTK_BOX(root), columns, TRUE, TRUE, 0);

    // Left: QR login.
    GtkWidget *left = gtk_box_new(GTK_ORIENTATION_VERTICAL, 12);
    gtk_widget_set_size_request(left, 330, -1);
    gtk_box_pack_start(GTK_BOX(left), label("扫码登录", "heading"), FALSE, FALSE, 0);
    GtkWidget *card = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    gtk_style_context_add_class(gtk_widget_get_style_context(card), "card");
    gtk_widget_set_halign(card, GTK_ALIGN_CENTER);
    gtk_widget_set_size_request(card, 252, 252);
    stack = gtk_stack_new();
    gtk_stack_set_transition_type(GTK_STACK(stack), GTK_STACK_TRANSITION_TYPE_CROSSFADE);
    GtkWidget *loading = gtk_box_new(GTK_ORIENTATION_VERTICAL, 10);
    gtk_widget_set_valign(loading, GTK_ALIGN_CENTER);
    GtkWidget *spinner = gtk_spinner_new();
    gtk_spinner_start(GTK_SPINNER(spinner));
    gtk_widget_set_size_request(spinner, 32, 32);
    gtk_box_pack_start(GTK_BOX(loading), spinner, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(loading), label("正在获取二维码…", "caption"), FALSE, FALSE, 0);
    gtk_stack_add_named(GTK_STACK(stack), loading, "loading");
    qr_image = gtk_image_new();
    gtk_stack_add_named(GTK_STACK(stack), qr_image, "qr");
    GtkWidget *done = gtk_box_new(GTK_ORIENTATION_VERTICAL, 4);
    gtk_widget_set_valign(done, GTK_ALIGN_CENTER);
    gtk_box_pack_start(GTK_BOX(done), label("✓", "done-mark"), FALSE, FALSE, 0);
    done_label = label("登录成功", "done-text");
    gtk_label_set_justify(GTK_LABEL(done_label), GTK_JUSTIFY_CENTER);
    gtk_label_set_line_wrap(GTK_LABEL(done_label), TRUE);
    gtk_box_pack_start(GTK_BOX(done), done_label, FALSE, FALSE, 0);
    gtk_stack_add_named(GTK_STACK(stack), done, "done");
    gtk_box_pack_start(GTK_BOX(card), stack, TRUE, TRUE, 0);
    gtk_box_pack_start(GTK_BOX(left), card, FALSE, FALSE, 0);
    GtkWidget *caption = label("请使用网易 UU 远程 App 扫一扫\n可快速登录", "caption");
    gtk_label_set_justify(GTK_LABEL(caption), GTK_JUSTIFY_CENTER);
    gtk_box_pack_start(GTK_BOX(left), caption, FALSE, FALSE, 0);
    status_label = label("", "status");
    gtk_label_set_line_wrap(GTK_LABEL(status_label), TRUE);
    gtk_label_set_justify(GTK_LABEL(status_label), GTK_JUSTIFY_CENTER);
    gtk_label_set_max_width_chars(GTK_LABEL(status_label), 34);
    gtk_box_pack_start(GTK_BOX(left), status_label, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(columns), left, TRUE, TRUE, 0);

    GtkWidget *sep = gtk_separator_new(GTK_ORIENTATION_VERTICAL);
    gtk_widget_set_margin_top(sep, 6);
    gtk_widget_set_margin_bottom(sep, 24);
    gtk_box_pack_start(GTK_BOX(columns), sep, FALSE, FALSE, 0);

    // Right: what will be registered.
    GtkWidget *right = gtk_box_new(GTK_ORIENTATION_VERTICAL, 14);
    gtk_widget_set_margin_start(right, 30);
    gtk_widget_set_size_request(right, 300, -1);
    GtkWidget *rh = label("本机信息", "heading");
    gtk_box_pack_start(GTK_BOX(right), rh, FALSE, FALSE, 0);
    GtkWidget *grid = gtk_grid_new();
    gtk_grid_set_row_spacing(GTK_GRID(grid), 12);
    gtk_grid_set_column_spacing(GTK_GRID(grid), 16);
    gtk_widget_set_margin_top(grid, 10);
    const char *keys[4] = {"设备名称", "操作系统", "处理器", "显卡"};
    for (int i = 0; i < 4; i++) info_row(GTK_GRID(grid), i, keys[i], info[i]);
    gtk_box_pack_start(GTK_BOX(right), grid, FALSE, FALSE, 0);
    GtkWidget *note = label("登录后可在 Mac / Windows / 手机上的官方 UU 远程客户端中连接本机的桌面、文件传输和终端。"
                            "登录信息只保存在本机配置文件中。", "note");
    gtk_label_set_line_wrap(GTK_LABEL(note), TRUE);
    gtk_label_set_max_width_chars(GTK_LABEL(note), 30);
    gtk_label_set_xalign(GTK_LABEL(note), 0);
    gtk_widget_set_margin_top(note, 10);
    gtk_box_pack_start(GTK_BOX(right), note, FALSE, FALSE, 0);
    done_button = gtk_button_new_with_label("完成");
    gtk_style_context_add_class(gtk_widget_get_style_context(done_button), "primary");
    gtk_widget_set_halign(done_button, GTK_ALIGN_START);
    gtk_widget_set_no_show_all(done_button, TRUE);
    g_signal_connect(done_button, "clicked", G_CALLBACK(on_done_clicked), NULL);
    gtk_box_pack_end(GTK_BOX(right), done_button, FALSE, FALSE, 20);
    gtk_box_pack_start(GTK_BOX(columns), right, TRUE, TRUE, 0);

    GtkWidget *footer = label("开源 Linux 主机端 · 非网易官方客户端", "footer");
    gtk_box_pack_end(GTK_BOX(root), footer, FALSE, FALSE, 0);
    gtk_widget_show_all(window);
}

static void *ui_main(void *arg) {
    (void)arg;
    int ok = gtk_init_check(NULL, NULL);
    if (ok) build_window();
    pthread_mutex_lock(&init_lock);
    init_state = ok ? 1 : -1;
    pthread_cond_signal(&init_cond);
    pthread_mutex_unlock(&init_lock);
    if (ok) gtk_main();
    else atomic_store(&closed, 1);
    return NULL;
}

// Returns 0 when the window is shown, -1 when no display is usable.
int uu_login_ui_start(const char *name, const char *os, const char *cpu, const char *gpu) {
    if (ui_started) return init_state == 1 ? 0 : -1;
    const char *src[4] = {name, os, cpu, gpu};
    for (int i = 0; i < 4; i++) info[i] = g_strdup(src[i] ? src[i] : "");
    if (pthread_create(&ui_thread, NULL, ui_main, NULL) != 0) return -1;
    ui_started = 1;
    pthread_mutex_lock(&init_lock);
    while (init_state == 0) pthread_cond_wait(&init_cond, &init_lock);
    pthread_mutex_unlock(&init_lock);
    if (init_state != 1) { pthread_join(ui_thread, NULL); return -1; }
    return 0;
}

static gboolean apply_qr(gpointer data) {
    QrUpdate *u = data;
    if (window) {
        GdkPixbuf *pb = gdk_pixbuf_new_from_data(u->rgba, GDK_COLORSPACE_RGB, TRUE, 8, u->edge, u->edge, u->edge * 4,
                                                 (GdkPixbufDestroyNotify)g_free, NULL);
        gtk_image_set_from_pixbuf(GTK_IMAGE(qr_image), pb);
        g_object_unref(pb);
        gtk_stack_set_visible_child_name(GTK_STACK(stack), "qr");
    } else g_free(u->rgba);
    g_free(u);
    return G_SOURCE_REMOVE;
}

void uu_login_ui_set_qr(const unsigned char *rgba, int edge) {
    if (atomic_load(&closed) || edge <= 0) return;
    QrUpdate *u = g_new(QrUpdate, 1);
    u->rgba = g_memdup2(rgba, (gsize)edge * edge * 4);
    u->edge = edge;
    g_idle_add(apply_qr, u);
}

static gboolean apply_status(gpointer data) {
    StatusUpdate *u = data;
    if (window) {
        GtkStyleContext *ctx = gtk_widget_get_style_context(status_label);
        gtk_style_context_remove_class(ctx, "ok");
        gtk_style_context_remove_class(ctx, "error");
        if (u->kind == 1) gtk_style_context_add_class(ctx, "ok");
        if (u->kind == 2) gtk_style_context_add_class(ctx, "error");
        gtk_label_set_text(GTK_LABEL(status_label), u->text);
        if (u->kind == 2 && !gtk_image_get_pixbuf(GTK_IMAGE(qr_image)))
            gtk_widget_hide(gtk_stack_get_child_by_name(GTK_STACK(stack), "loading"));
    }
    g_free(u->text);
    g_free(u);
    return G_SOURCE_REMOVE;
}

// kind: 0 info, 1 success, 2 error.
void uu_login_ui_set_status(const char *text, int kind) {
    if (atomic_load(&closed)) return;
    StatusUpdate *u = g_new(StatusUpdate, 1);
    u->text = g_strdup(text ? text : "");
    u->kind = kind;
    g_idle_add(apply_status, u);
}

static gboolean apply_done(gpointer data) {
    if (window) {
        gtk_label_set_text(GTK_LABEL(done_label), data);
        gtk_stack_set_visible_child_name(GTK_STACK(stack), "done");
        gtk_widget_show(done_button);
        gtk_widget_grab_focus(done_button);
    }
    g_free(data);
    return G_SOURCE_REMOVE;
}

void uu_login_ui_set_done(const char *text) {
    if (atomic_load(&closed)) return;
    g_idle_add(apply_done, g_strdup(text ? text : ""));
}

int uu_login_ui_closed(void) { return atomic_load(&closed); }

static gboolean apply_close(gpointer data) {
    (void)data;
    if (window) gtk_widget_destroy(window);
    return G_SOURCE_REMOVE;
}

void uu_login_ui_close(void) {
    if (!ui_started) return;
    if (!atomic_load(&closed)) g_idle_add(apply_close, NULL);
    pthread_join(ui_thread, NULL);
    ui_started = 0;
}
