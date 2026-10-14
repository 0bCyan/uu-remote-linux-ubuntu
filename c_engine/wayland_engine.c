/* Native Hyprland/wlroots desktop bridge. Protocol XMLs retain upstream licenses. */
#define _GNU_SOURCE
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>
#include <pthread.h>
#include <poll.h>
#include <sys/mman.h>
#include <unistd.h>
#include <time.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include "wlr-screencopy-unstable-v1-client-protocol.h"
#include "wlr-virtual-pointer-unstable-v1-client-protocol.h"
#include "virtual-keyboard-unstable-v1-client-protocol.h"
#include "wlr-output-power-management-unstable-v1-client-protocol.h"

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static struct wl_display *display;
static struct wl_shm *shm;
static struct wl_seat *seat;
static struct zwlr_screencopy_manager_v1 *copy_manager;
static struct zwlr_virtual_pointer_manager_v1 *pointer_manager;
static struct zwp_virtual_keyboard_manager_v1 *keyboard_manager;
static struct zwlr_virtual_pointer_v1 *pointer;
static struct zwp_virtual_keyboard_v1 *keyboard;
static struct zwlr_output_power_manager_v1 *power_manager;
static struct zwlr_output_power_v1 *power;
static int power_failed;
static struct xkb_context *xkb;
static struct xkb_keymap *keymap;
static struct xkb_state *key_state;
static unsigned char held_keys[768];
struct output { struct wl_output *handle; char name[128]; int transform,scale; uint32_t width,height; };
static struct output outputs[32];
static unsigned output_count, selected;
static uint32_t width,height,stride,format,flags;
static int done,failed,initialized;
static struct wl_buffer *buffer;
static void *pixels;
static size_t pixel_length;

static uint32_t timestamp(void) {
    struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t);
    return (uint32_t)((uint64_t)t.tv_sec*1000+t.tv_nsec/1000000);
}
/* Never let a disappearing compositor block a worker indefinitely. */
static int dispatch_until(int *condition) {
    uint32_t start=timestamp();
    while (!*condition) {
        while (wl_display_prepare_read(display)!=0) {
            if (wl_display_dispatch_pending(display)<0) return -1;
            if (*condition) return 0;
        }
        if (wl_display_flush(display)<0) { wl_display_cancel_read(display); return -1; }
        struct pollfd fd={wl_display_get_fd(display),POLLIN,0};
        int wait=poll(&fd,1,100);
        if (wait>0 && (fd.revents&POLLIN)) {
            if (wl_display_read_events(display)<0) return -1;
            if (wl_display_dispatch_pending(display)<0) return -1;
        } else { wl_display_cancel_read(display); if (wait<0 || fd.revents&(POLLERR|POLLHUP)) return -1; }
        if ((uint32_t)(timestamp()-start)>3000) return -1;
    }
    return 0;
}
static void sync_done(void *data,struct wl_callback *cb,uint32_t serial) { *(int*)data=1; wl_callback_destroy(cb); }
static const struct wl_callback_listener sync_listener={sync_done};
static int sync_display(void) {
    int ready=0; struct wl_callback *cb=wl_display_sync(display);
    wl_callback_add_listener(cb,&sync_listener,&ready);
    int result=dispatch_until(&ready);
    if (!ready) wl_callback_destroy(cb);
    return result;
}
static void output_geometry(void*d,struct wl_output*o,int32_t x,int32_t y,int32_t w,int32_t h,int32_t sub,const char*m,const char*model,int32_t transform) { ((struct output*)d)->transform=transform; }
static void output_mode(void*d,struct wl_output*o,uint32_t f,int32_t w,int32_t h,int32_t rate) {
    if(f&WL_OUTPUT_MODE_CURRENT) { ((struct output*)d)->width=w; ((struct output*)d)->height=h; }
}
static void output_done(void*d,struct wl_output*o) {}
static void output_scale(void*d,struct wl_output*o,int32_t scale) { if(scale>0 && scale<=16) ((struct output*)d)->scale=scale; }
static void output_name(void*d,struct wl_output*o,const char*name) { snprintf(((struct output*)d)->name,128,"%s",name); }
static void output_description(void*d,struct wl_output*o,const char*desc) {}
static const struct wl_output_listener output_listener={output_geometry,output_mode,output_done,output_scale,output_name,output_description};
static void global(void*d,struct wl_registry*r,uint32_t id,const char*interface,uint32_t version) {
    if (!strcmp(interface,"wl_shm")) shm=wl_registry_bind(r,id,&wl_shm_interface,1);
    else if (!strcmp(interface,"wl_seat") && !seat) seat=wl_registry_bind(r,id,&wl_seat_interface,1);
    else if (!strcmp(interface,"wl_output") && output_count<32) {
        struct output *o=&outputs[output_count++];
        o->scale=1;
        o->handle=wl_registry_bind(r,id,&wl_output_interface,version<4?version:4);
        wl_output_add_listener(o->handle,&output_listener,o);
    } else if (!strcmp(interface,"zwlr_screencopy_manager_v1")) copy_manager=wl_registry_bind(r,id,&zwlr_screencopy_manager_v1_interface,version<3?version:3);
    else if (!strcmp(interface,"zwlr_virtual_pointer_manager_v1") && version>=2) pointer_manager=wl_registry_bind(r,id,&zwlr_virtual_pointer_manager_v1_interface,2);
    else if (!strcmp(interface,"zwp_virtual_keyboard_manager_v1")) keyboard_manager=wl_registry_bind(r,id,&zwp_virtual_keyboard_manager_v1_interface,1);
    else if (!strcmp(interface,"zwlr_output_power_manager_v1")) power_manager=wl_registry_bind(r,id,&zwlr_output_power_manager_v1_interface,1);
}
static void global_remove(void*d,struct wl_registry*r,uint32_t id) { /* failed captures are returned to caller */ }
static const struct wl_registry_listener registry_listener={global,global_remove};
static void power_mode(void*d,struct zwlr_output_power_v1*p,uint32_t mode) {}
static void power_failure(void*d,struct zwlr_output_power_v1*p) { power_failed=1; }
static const struct zwlr_output_power_v1_listener power_listener={power_mode,power_failure};
static void copy_buffer(void*d,struct zwlr_screencopy_frame_v1*f,uint32_t fmt,uint32_t w,uint32_t h,uint32_t s) {
    format=fmt; width=w; height=h; stride=s;
    if (w==0 || h==0 || w>16384 || h>16384 || s<w*4 || s>131072 || (uint64_t)s*h>256*1024*1024 ||
        (fmt!=WL_SHM_FORMAT_XRGB8888 && fmt!=WL_SHM_FORMAT_ARGB8888 &&
         fmt!=WL_SHM_FORMAT_XBGR8888 && fmt!=WL_SHM_FORMAT_ABGR8888)) { failed=done=1; return; }
    size_t length=(size_t)s*h;
    if (buffer) { wl_buffer_destroy(buffer); buffer=NULL; }
    if (pixels) { munmap(pixels,pixel_length); pixels=NULL; }
    int fd=memfd_create("uuremote-wayland-frame",MFD_CLOEXEC);
    if (fd<0 || ftruncate(fd,(off_t)length)!=0) { if(fd>=0)close(fd); failed=done=1; return; }
    pixels=mmap(NULL,length,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);
    if (pixels==MAP_FAILED) { pixels=NULL; close(fd); failed=done=1; return; }
    pixel_length=length;
    struct wl_shm_pool *pool=wl_shm_create_pool(shm,fd,(int32_t)length);
    buffer=wl_shm_pool_create_buffer(pool,0,w,h,s,fmt);
    wl_shm_pool_destroy(pool); close(fd);
    /* copy is valid immediately for SHM, including frame versions <3. */
    if (zwlr_screencopy_frame_v1_get_version(f)<3) zwlr_screencopy_frame_v1_copy(f,buffer);
}
static void copy_flags(void*d,struct zwlr_screencopy_frame_v1*f,uint32_t value) { flags=value; }
static void copy_ready(void*d,struct zwlr_screencopy_frame_v1*f,uint32_t hi,uint32_t lo,uint32_t ns) { done=1; }
static void copy_failed(void*d,struct zwlr_screencopy_frame_v1*f) { failed=done=1; }
static void copy_damage(void*d,struct zwlr_screencopy_frame_v1*f,uint32_t x,uint32_t y,uint32_t w,uint32_t h) {}
static void copy_dmabuf(void*d,struct zwlr_screencopy_frame_v1*f,uint32_t fmt,uint32_t w,uint32_t h) {}
static void copy_buffer_done(void*d,struct zwlr_screencopy_frame_v1*f) { if(buffer && !failed) zwlr_screencopy_frame_v1_copy(f,buffer); else failed=done=1; }
static const struct zwlr_screencopy_frame_v1_listener copy_listener={copy_buffer,copy_flags,copy_ready,copy_failed,copy_damage,copy_dmabuf,copy_buffer_done};
static int capture(void) {
    done=failed=0; flags=0;
    if(buffer) { wl_buffer_destroy(buffer); buffer=NULL; }
    /* Initial milestone: video excludes cursor, viewer draws its own pointer. */
    struct zwlr_screencopy_frame_v1 *f=zwlr_screencopy_manager_v1_capture_output(copy_manager,0,outputs[selected].handle);
    zwlr_screencopy_frame_v1_add_listener(f,&copy_listener,NULL);
    int result=dispatch_until(&done);
    zwlr_screencopy_frame_v1_destroy(f);
    return result==0 && !failed && pixels ? 0:-1;
}
static void reset(void) {
    if (key_state) xkb_state_unref(key_state);
    if (keymap) xkb_keymap_unref(keymap);
    if (xkb) xkb_context_unref(xkb);
    if (pixels) munmap(pixels,pixel_length);
    /* Disconnect drops every proxy; failure never leaves an initialized backend. */
    if (display) wl_display_disconnect(display);
    display=NULL; shm=NULL; seat=NULL; copy_manager=NULL; pointer_manager=NULL; keyboard_manager=NULL;
    pointer=NULL; keyboard=NULL; xkb=NULL; keymap=NULL; key_state=NULL; buffer=NULL; pixels=NULL;
    power_manager=NULL; power=NULL; power_failed=0;
    pixel_length=0; output_count=0; initialized=0; memset(outputs,0,sizeof(outputs));
    memset(held_keys,0,sizeof(held_keys));
}
int uu_wl_init(int32_t *w,int32_t *h) {
    pthread_mutex_lock(&lock);
    if(initialized) { *w=width; *h=height; pthread_mutex_unlock(&lock); return 0; }
    int result=-1;
    display=wl_display_connect(NULL); if (!display) goto end;
    struct wl_registry *registry=wl_display_get_registry(display);
    wl_registry_add_listener(registry,&registry_listener,NULL);
    if(sync_display()!=0 || sync_display()!=0) goto end;
    if(!shm || !seat || !copy_manager || !pointer_manager || !keyboard_manager || !output_count) goto end;
    const char *name=getenv("UU_WAYLAND_OUTPUT"); selected=0;
    if(name && *name) {
        for(selected=0;selected<output_count;selected++) if(!strcmp(outputs[selected].name,name)) break;
        if(selected==output_count) goto end;
    }
    /* Rotated output coordinate mapping needs separate acceptance. */
    if(outputs[selected].transform!=WL_OUTPUT_TRANSFORM_NORMAL) goto end;
    width=outputs[selected].width; height=outputs[selected].height;
    if(!width || !height || width>16384 || height>16384 || (uint64_t)width*height*4>256*1024*1024) goto end;
    if(power_manager) {
        power=zwlr_output_power_manager_v1_get_output_power(power_manager,outputs[selected].handle);
        zwlr_output_power_v1_add_listener(power,&power_listener,NULL);
    }
    pointer=zwlr_virtual_pointer_manager_v1_create_virtual_pointer_with_output(pointer_manager,seat,outputs[selected].handle);
    keyboard=zwp_virtual_keyboard_manager_v1_create_virtual_keyboard(keyboard_manager,seat);
    xkb=xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    if(!xkb) goto end;
    struct xkb_rule_names names={.layout="us"};
    keymap=xkb_keymap_new_from_names(xkb,&names,XKB_KEYMAP_COMPILE_NO_FLAGS);
    if(!keymap) goto end;
    key_state=xkb_state_new(keymap); if(!key_state) goto end;
    char *map=xkb_keymap_get_as_string(keymap,XKB_KEYMAP_FORMAT_TEXT_V1);
    if(!map) goto end;
    size_t length=strlen(map)+1;
    int fd=memfd_create("uuremote-keymap",MFD_CLOEXEC);
    if(fd<0) { free(map); goto end; }
    if(ftruncate(fd,(off_t)length)!=0 || write(fd,map,length)!=(ssize_t)length) { free(map); close(fd); goto end; }
    free(map);
    zwp_virtual_keyboard_v1_keymap(keyboard,WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1,fd,length); close(fd);
    if(sync_display()!=0) goto end;
    initialized=1; *w=width; *h=height; result=0;
end:
    if(result!=0) reset();
    pthread_mutex_unlock(&lock); return result;
}
int uu_wl_wake(void) {
    pthread_mutex_lock(&lock); int result=-1;
    if(initialized && power && !power_failed) {
        zwlr_output_power_v1_set_mode(power,ZWLR_OUTPUT_POWER_V1_MODE_ON);
        if(sync_display()==0 && !power_failed) result=0;
    }
    pthread_mutex_unlock(&lock); return result;
}
int uu_wl_capture(uint8_t *out,size_t length) {
    pthread_mutex_lock(&lock);
    int result=-1;
    if(initialized && capture()==0 && length==(size_t)width*height*4) {
        for(uint32_t y=0;y<height;y++) {
            uint32_t source=(flags&ZWLR_SCREENCOPY_FRAME_V1_FLAGS_Y_INVERT)?height-y-1:y;
            uint8_t *row=out+(size_t)y*width*4;
            memcpy(row,(uint8_t*)pixels+(size_t)source*stride,width*4);
            if(format==WL_SHM_FORMAT_XBGR8888 || format==WL_SHM_FORMAT_ABGR8888) {
                for(uint32_t x=0;x<width;x++) { uint8_t red=row[x*4]; row[x*4]=row[x*4+2]; row[x*4+2]=red; }
            }
        }
        result=0;
    }
    pthread_mutex_unlock(&lock); return result;
}
int uu_wl_move(int32_t x,int32_t y,int relative) {
    pthread_mutex_lock(&lock); int result=-1;
    if(initialized) {
        if(relative) {
            /* UU reports pixels (source DPR=1); Wayland motion is in logical points. */
            double scale=outputs[selected].scale;
            zwlr_virtual_pointer_v1_motion(pointer,timestamp(),wl_fixed_from_double(x/scale),wl_fixed_from_double(y/scale));
        }
        else if(x>=0 && y>=0 && x<(int32_t)width && y<(int32_t)height) zwlr_virtual_pointer_v1_motion_absolute(pointer,timestamp(),x,y,width,height);
        else goto end;
        zwlr_virtual_pointer_v1_frame(pointer); result=wl_display_flush(display)<0?-1:0;
    }
end: pthread_mutex_unlock(&lock); return result;
}
int uu_wl_button(int button,int pressed) {
    pthread_mutex_lock(&lock); int result=-1;
    if(initialized) {
        if(button>=4 && button<=7) {
            if(pressed) {
                uint32_t axis=(button<=5)?WL_POINTER_AXIS_VERTICAL_SCROLL:WL_POINTER_AXIS_HORIZONTAL_SCROLL;
                int direction=(button==4 || button==6)?-1:1;
                zwlr_virtual_pointer_v1_axis_source(pointer,WL_POINTER_AXIS_SOURCE_WHEEL);
                zwlr_virtual_pointer_v1_axis_discrete(pointer,timestamp(),axis,wl_fixed_from_int(direction*10),direction);
            }
        } else {
            int code=button==1?0x110:button==2?0x112:button==3?0x111:button==8?0x113:button==9?0x114:0;
            if(!code) goto end;
            zwlr_virtual_pointer_v1_button(pointer,timestamp(),code,pressed?WL_POINTER_BUTTON_STATE_PRESSED:WL_POINTER_BUTTON_STATE_RELEASED);
        }
        zwlr_virtual_pointer_v1_frame(pointer); result=wl_display_flush(display)<0?-1:0;
    }
end: pthread_mutex_unlock(&lock); return result;
}
int uu_wl_key(uint64_t symbol,int pressed) {
    pthread_mutex_lock(&lock); int result=-1;
    if(initialized) {
        for(xkb_keycode_t code=xkb_keymap_min_keycode(keymap);code<=xkb_keymap_max_keycode(keymap);code++) {
            int matches=0;
            for(xkb_level_index_t level=0;level<xkb_keymap_num_levels_for_key(keymap,code,0);level++) {
                const xkb_keysym_t *syms;
                int count=xkb_keymap_key_get_syms_by_level(keymap,code,0,level,&syms);
                if(count==1 && syms[0]==symbol) { matches=1; break; }
            }
            if(!matches || code<8 || code>=sizeof(held_keys)) continue;
            if(held_keys[code]==!!pressed) { result=0; break; }
            held_keys[code]=!!pressed;
            zwp_virtual_keyboard_v1_key(keyboard,timestamp(),code-8,pressed?WL_KEYBOARD_KEY_STATE_PRESSED:WL_KEYBOARD_KEY_STATE_RELEASED);
            xkb_state_update_key(key_state,code,pressed?XKB_KEY_DOWN:XKB_KEY_UP);
            zwp_virtual_keyboard_v1_modifiers(keyboard,
                xkb_state_serialize_mods(key_state,XKB_STATE_MODS_DEPRESSED),
                xkb_state_serialize_mods(key_state,XKB_STATE_MODS_LATCHED),
                xkb_state_serialize_mods(key_state,XKB_STATE_MODS_LOCKED),
                xkb_state_serialize_layout(key_state,XKB_STATE_LAYOUT_EFFECTIVE));
            result=wl_display_flush(display)<0?-1:0; break;
        }
    }
    pthread_mutex_unlock(&lock); return result;
}
