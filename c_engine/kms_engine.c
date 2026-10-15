/* Opt-in KMS prototype: read active primary-plane linear XRGB/ARGB buffers.
 * No modesetting, DRM master acquisition, authentication or content logging.
 */
#include <xf86drm.h>
#include <xf86drmMode.h>
#include <drm_fourcc.h>
#include <sys/mman.h>
#include <fcntl.h>
#include <unistd.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <errno.h>
struct uu_kms { int fd; uint32_t crtc, plane, width, height; };
static uint32_t primary_plane(int fd, uint32_t crtc_id) {
    drmModePlaneRes *planes=drmModeGetPlaneResources(fd);
    if (!planes) return 0;
    uint32_t selected=0;
    for (uint32_t i=0;i<planes->count_planes && !selected;i++) {
        drmModePlane *plane=drmModeGetPlane(fd,planes->planes[i]);
        if (plane && plane->crtc_id==crtc_id && plane->fb_id) {
            drmModeObjectProperties *props=drmModeObjectGetProperties(fd,plane->plane_id,DRM_MODE_OBJECT_PLANE);
            if (props) {
                for (uint32_t j=0;j<props->count_props;j++) {
                    drmModePropertyRes *prop=drmModeGetProperty(fd,props->props[j]);
                    if (prop && strcmp(prop->name,"type")==0 && props->prop_values[j]==DRM_PLANE_TYPE_PRIMARY) selected=plane->plane_id;
                    if (prop) drmModeFreeProperty(prop);
                }
                drmModeFreeObjectProperties(props);
            }
        }
        if (plane) drmModeFreePlane(plane);
    }
    drmModeFreePlaneResources(planes);
    return selected;
}
static void close_handles(int fd, drmModeFB2 *fb) {
    for (int i=0;i<4;i++) {
        if (!fb->handles[i]) continue;
        int duplicate=0;
        for (int j=0;j<i;j++) if (fb->handles[j]==fb->handles[i]) duplicate=1;
        if (!duplicate) { struct drm_gem_close c={.handle=fb->handles[i]}; drmIoctl(fd,DRM_IOCTL_GEM_CLOSE,&c); }
    }
}
void uu_kms_close(struct uu_kms *s) { if (s) { close(s->fd); free(s); } }
struct uu_kms *uu_kms_open(const char *device, uint32_t *w, uint32_t *h) {
    int fd=open(device,O_RDWR|O_CLOEXEC);
    if (fd<0) return NULL;
    if (drmSetClientCap(fd,DRM_CLIENT_CAP_UNIVERSAL_PLANES,1)<0) { close(fd); return NULL; }
    drmModeRes *res=drmModeGetResources(fd);
    if (!res) { close(fd); return NULL; }
    struct uu_kms *s=NULL;
    for (int i=0;i<res->count_crtcs;i++) {
        drmModeCrtc *crtc=drmModeGetCrtc(fd,res->crtcs[i]);
        uint32_t plane=crtc ? primary_plane(fd,crtc->crtc_id) : 0;
        if (crtc && plane && crtc->mode_valid && crtc->width && crtc->height && crtc->width<=16384 && crtc->height<=16384) {
            s=calloc(1,sizeof(*s));
            if (s) { s->fd=fd; s->crtc=crtc->crtc_id; s->plane=plane; s->width=crtc->width; s->height=crtc->height; *w=s->width; *h=s->height; }
        }
        if (crtc) drmModeFreeCrtc(crtc);
        if (s) break;
    }
    drmModeFreeResources(res);
    if (!s) close(fd);
    return s;
}
int uu_kms_read(struct uu_kms *s, uint8_t *out, size_t length) {
    drmModeCrtc *crtc=drmModeGetCrtc(s->fd,s->crtc);
    if (!crtc) return -1;
    drmModePlane *plane=drmModeGetPlane(s->fd,s->plane);
    uint32_t id=plane ? plane->fb_id : 0;
    if (plane) drmModeFreePlane(plane);
    int valid=crtc->mode_valid && crtc->width==s->width && crtc->height==s->height;
    drmModeFreeCrtc(crtc);
    if (!valid || !id) return -2; /* mode changed: caller must reopen */
    drmModeFB2 *fb=drmModeGetFB2(s->fd,id);
    if (!fb) return -3;
    int result=-4;
    if (!fb->handles[0]) { result=-5; goto done; } /* DRM master or CAP_SYS_ADMIN needed */
    if (fb->width!=s->width || fb->height!=s->height ||
        (fb->pixel_format!=DRM_FORMAT_XRGB8888 && fb->pixel_format!=DRM_FORMAT_ARGB8888) ||
        ((fb->flags & DRM_MODE_FB_MODIFIERS) && fb->modifier!=DRM_FORMAT_MOD_LINEAR) ||
        fb->handles[1] || fb->pitches[0] < s->width*4 || length!=(size_t)s->width*s->height*4) goto done;
    struct drm_mode_map_dumb map={.handle=fb->handles[0]};
    if (drmIoctl(s->fd,DRM_IOCTL_MODE_MAP_DUMB,&map)<0) { result=-6; goto done; }
    size_t bytes=(size_t)fb->pitches[0]*fb->height+fb->offsets[0];
    void *data=mmap(NULL,bytes,PROT_READ,MAP_SHARED,s->fd,map.offset);
    if (data==MAP_FAILED) { result=-7; goto done; }
    for (uint32_t y=0;y<s->height;y++) {
        uint8_t *row=out+(size_t)y*s->width*4;
        memcpy(row,(uint8_t*)data+fb->offsets[0]+(size_t)y*fb->pitches[0],s->width*4);
        for (uint32_t x=0;x<s->width;x++) row[x*4+3]=255;
    }
    munmap(data,bytes);
    result=0;
done:
    close_handles(s->fd,fb);
    drmModeFreeFB2(fb);
    return result;
}
