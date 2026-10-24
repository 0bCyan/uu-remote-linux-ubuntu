#include "dri3_capture.c"
static int equal_images(Dri3Capture*c){
 xcb_get_image_reply_t*a=xcb_get_image_reply(c->conn,xcb_get_image(c->conn,XCB_IMAGE_FORMAT_Z_PIXMAP,c->screen->root,0,0,c->width,c->height,~0u),NULL);
 xcb_get_image_reply_t*b=xcb_get_image_reply(c->conn,xcb_get_image(c->conn,XCB_IMAGE_FORMAT_Z_PIXMAP,c->pix,0,0,c->width,c->height,~0u),NULL);
 int ok=a&&b&&xcb_get_image_data_length(a)==xcb_get_image_data_length(b)&&!memcmp(xcb_get_image_data(a),xcb_get_image_data(b),xcb_get_image_data_length(a));free(a);free(b);return ok;
}
static void draw(Dri3Capture*c,uint32_t color,xcb_rectangle_t*r,int n){xcb_change_gc(c->conn,c->gc,XCB_GC_FOREGROUND,&color);for(int i=0;i<n;i++)xcb_poly_fill_rectangle(c->conn,c->screen->root,c->gc,1,r+i);xcb_flush(c->conn);}
#define CHECK(x) do{if(!(x)){fprintf(stderr,"FAIL line=%d expr=%s copied=%llu\n",__LINE__,#x,(unsigned long long)c.copied_pixels);return 1;}}while(0)
int main(){
 Dri3Capture c={0};c.conn=xcb_connect(NULL,NULL);CHECK(!xcb_connection_has_error(c.conn));c.screen=xcb_setup_roots_iterator(xcb_get_setup(c.conn)).data;c.width=c.screen->width_in_pixels;c.height=c.screen->height_in_pixels;c.needs_full_copy=1;
 xcb_damage_query_version_reply_t*d=xcb_damage_query_version_reply(c.conn,xcb_damage_query_version(c.conn,1,1),NULL);CHECK(d);free(d);
 xcb_xfixes_query_version_reply_t*f=xcb_xfixes_query_version_reply(c.conn,xcb_xfixes_query_version(c.conn,2,0),NULL);CHECK(f);free(f);
 c.pix=xcb_generate_id(c.conn);xcb_create_pixmap(c.conn,c.screen->root_depth,c.pix,c.screen->root,c.width,c.height);c.gc=xcb_generate_id(c.conn);uint32_t mode=XCB_SUBWINDOW_MODE_INCLUDE_INFERIORS;xcb_create_gc(c.conn,c.gc,c.pix,XCB_GC_SUBWINDOW_MODE,&mode);
 c.damage=xcb_generate_id(c.conn);c.damage_event_code=xcb_get_extension_data(c.conn,&xcb_damage_id)->first_event;xcb_damage_create(c.conn,c.damage,c.screen->root,XCB_DAMAGE_REPORT_LEVEL_NON_EMPTY);c.dirty_region=xcb_generate_id(c.conn);xcb_xfixes_create_region(c.conn,c.dirty_region,0,NULL);
 xcb_rectangle_t all={0,0,c.width,c.height};draw(&c,0x223344,&all,1);CHECK(dri3_capture_blit(&c)==0);CHECK(equal_images(&c));CHECK(c.copied_pixels==(uint64_t)c.width*c.height);
 // Consume initial notifications; their region is already copied.
 dri3_capture_wait_damage(&c,0);CHECK(!dri3_capture_blit(&c));CHECK(c.copied_pixels==0);
 xcb_rectangle_t small[2]={{40,70,10,20},{160,240,15,16}};draw(&c,0xaabbcc,small,2);CHECK(dri3_capture_wait_damage(&c,1000)==1);CHECK(!dri3_capture_blit(&c));CHECK(equal_images(&c));CHECK(c.copied_pixels==440);printf("small_updates_pixels=%llu full_pixels=%u unchanged_pixels_preserved=true\n",(unsigned long long)c.copied_pixels,c.width*c.height);
 xcb_rectangle_t edge={c.width-30,c.height-10,50,40};draw(&c,0x445566,&edge,1);CHECK(dri3_capture_wait_damage(&c,1000)==1);CHECK(!dri3_capture_blit(&c));CHECK(equal_images(&c));CHECK(c.copied_pixels==300);
 draw(&c,0x112233,&all,1);CHECK(dri3_capture_wait_damage(&c,1000)==1);CHECK(!dri3_capture_blit(&c));CHECK(equal_images(&c));CHECK(c.copied_pixels==(uint64_t)c.width*c.height);
 CHECK(!dri3_capture_blit(&c));CHECK(c.copied_pixels==0);CHECK(equal_images(&c));
 xcb_xfixes_destroy_region(c.conn,c.dirty_region);c.dirty_region=0;draw(&c,0x334455,small,2);CHECK(!dri3_capture_blit(&c));CHECK(equal_images(&c));CHECK(c.copied_pixels==(uint64_t)c.width*c.height);
 printf("initial_full=true partial=true edge_clip=true large_damage_full=true empty_no_copy=true full_copy_fallback=true result=PASS\n");xcb_disconnect(c.conn);return 0;
}
