#include "vaapi_engine.h"
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>
#include <gbm.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/Xlib-xcb.h>
#include <xcb/dri3.h>
int main(int argc,char**argv){
 if(argc!=7)return 2;
 // Generated BGRA only. DMA mode draws to an offscreen X11 pixmap, never the desktop.
 unsigned w=atoi(argv[1]),h=atoi(argv[2]),n=atoi(argv[3]);
 if(getenv("UU_PROBE_DYNAMIC") && (w<528 || h<528))return 2;
 unsigned motion=getenv("UU_PROBE_MOTION") ? atoi(getenv("UU_PROBE_MOTION")) : 0;
 if(motion==1)motion=768;
 if(motion && (motion<64 || motion>768 || w<400+motion || h<400+motion))return 2;
 FILE*in=fopen(argv[4],"rb");FILE*out=fopen(argv[5],"wb");if(!in||!out)return 3;
 unsigned char*p=malloc((size_t)w*h*4);if(fread(p,4,(size_t)w*h,in)!=(size_t)w*h)return 4;fclose(in);
 VaapiConfig c={.width=w,.height=h,.cap_width=w,.cap_height=h,.fps=30,.gop=60,.target_bps=6000000,.max_bps=8000000,.buffer_bits=4000000,.is_hevc=atoi(argv[6])};
 VaapiEngine*e=vaapi_create(&c);if(!e)return 5;
 Display*xd=NULL; xcb_pixmap_t pix=0; GC gc=NULL;
 int fd=-1,dma=-1;struct gbm_device*g=NULL;struct gbm_bo*bo=NULL;
 if(getenv("UU_PROBE_DMA")){
  fd=open("/dev/dri/renderD128",O_RDWR);g=gbm_create_device(fd);if(!g)return 8;
  bo=gbm_bo_create(g,w,h,GBM_FORMAT_XRGB8888,GBM_BO_USE_RENDERING|GBM_BO_USE_LINEAR);if(!bo)return 9;
  unsigned stride=gbm_bo_get_stride(bo);fprintf(stderr,"modifier=%llu stride=%u planes=%d\n",(unsigned long long)gbm_bo_get_modifier(bo),stride,gbm_bo_get_plane_count(bo));dma=gbm_bo_get_fd(bo);
  xd=XOpenDisplay(NULL);if(!xd)return 20;
  xcb_connection_t*xc=XGetXCBConnection(xd);xcb_screen_t*sc=xcb_setup_roots_iterator(xcb_get_setup(xc)).data;
  xcb_dri3_query_version_reply_t*v=xcb_dri3_query_version_reply(xc,xcb_dri3_query_version(xc,1,2),NULL);if(!v)return 21;free(v);
  pix=xcb_generate_id(xc);xcb_generic_error_t*err=xcb_request_check(xc,xcb_dri3_pixmap_from_buffer_checked(xc,pix,sc->root,stride*h,w,h,stride,sc->root_depth,32,dup(dma)));if(err)return 22;
  gc=XCreateGC(xd,pix,0,NULL);XImage*im=XCreateImage(xd,DefaultVisual(xd,DefaultScreen(xd)),24,ZPixmap,0,(char*)p,w,h,32,w*4);
  XPutImage(xd,pix,gc,im,0,0,0,0,w,h);XSync(xd,False);im->data=NULL;XDestroyImage(im);
  if(vaapi_import_dma_buffer(e,dma,stride))return 11;
 }else if(vaapi_upload_host_buffer(e,p,w*4))return 6;

 struct timespec start,end;clock_gettime(CLOCK_MONOTONIC,&start);size_t bytes=0;
 for(unsigned i=0;i<n;i++){
  if (getenv("UU_PROBE_RATE_STEPS") && (i==50 || i==110)) {
   long rate=i==50 ? 2000000 : 6000000;
   if(vaapi_set_bitrate(e,rate*3/4,rate,rate/4))return 25;
   fprintf(stderr,"live_rate_update frame=%u bps=%ld\n",i,rate);
  }
  // Generated high-complexity motion for loaded rate-control measurements.
  if (motion) {
   unsigned seed=i+1;
   for(unsigned y=400;y<400+motion;y++)for(unsigned x=400;x<400+motion;x++){
    seed=seed*1664525u+1013904223u;size_t k=((size_t)y*w+x)*4;
    p[k]=seed>>24;p[k+1]=seed>>16;p[k+2]=seed>>8;
   }
   if(xd){
    XImage*im=XCreateImage(xd,DefaultVisual(xd,DefaultScreen(xd)),24,ZPixmap,0,(char*)p,w,h,32,w*4);
    XPutImage(xd,pix,gc,im,400,400,400,400,motion,motion);XSync(xd,False);im->data=NULL;XDestroyImage(im);vaapi_mark_input_dirty(e);
   }else if(vaapi_upload_host_buffer(e,p,w*4))return 26;
  }
  if (getenv("UU_PROBE_DYNAMIC") && (i==60 || i==120)) {
   unsigned color = i==60 ? 0xff0000 : 0x00ff00;
   if (xd) { XSetForeground(xd,gc,color); XFillRectangle(xd,pix,gc,400,400,128,128); XSync(xd,False); vaapi_mark_input_dirty(e); }
   else { for(unsigned y=400;y<528;y++)for(unsigned x=400;x<528;x++){size_t k=((size_t)y*w+x)*4;p[k]=color;p[k+1]=color>>8;p[k+2]=color>>16;}
    if(vaapi_upload_host_buffer(e,p,w*4))return 14; }
  }
  const unsigned char*data=NULL;unsigned len=0;int r=vaapi_encode_frame(e,(i==37 || i==125),&data,&len);
  if(r||!len){fprintf(stderr,"encode failed frame=%u status=%d bytes=%u\n",i,r,len);return 7;}

  fwrite(data,1,len,out);bytes+=len;fprintf(stderr,"frame=%u bytes=%u requested_idr=%d\n",i,len,(i==37 || i==125));
 }
 clock_gettime(CLOCK_MONOTONIC,&end);double secs=end.tv_sec-start.tv_sec+(end.tv_nsec-start.tv_nsec)/1e9;
 printf("frames=%u bytes=%zu encode_seconds=%.3f fps=%.2f\n",n,bytes,secs,n/secs);fclose(out);vaapi_destroy(e);if(xd){XFreeGC(xd,gc);XFreePixmap(xd,pix);XCloseDisplay(xd);}if(dma>=0)close(dma);if(bo)gbm_bo_destroy(bo);if(g)gbm_device_destroy(g);if(fd>=0)close(fd);free(p);return 0;
}
