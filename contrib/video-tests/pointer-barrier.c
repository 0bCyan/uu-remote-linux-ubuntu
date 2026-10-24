// Only run on an isolated Xvfb: this moves the pointer and creates test barriers.
#include "x11_engine.c"
#include <X11/extensions/XInput2.h>
static int hits(Display*d,int op){int n=0;XSync(d,False);usleep(30000);while(XPending(d)){XEvent ev;XNextEvent(d,&ev);if(ev.type==GenericEvent&&ev.xcookie.extension==op&&XGetEventData(d,&ev.xcookie)){if(ev.xcookie.evtype==XI_BarrierHit)n++;XFreeEventData(d,&ev.xcookie);}}return n;}
#define CHECK(v) do{if(!(v)){fprintf(stderr,"FAIL line=%d\n",__LINE__);return 1;}}while(0)
int main(){CHECK(x11_init(NULL)==0);Display*d=g_display;int op,a,b;CHECK(XQueryExtension(d,"XInputExtension",&op,&a,&b));a=2;b=3;CHECK(XIQueryVersion(d,&a,&b)==Success);unsigned char m[XIMaskLen(XI_BarrierHit)]={0};XISetMask(m,XI_BarrierHit);XIEventMask mask={XIAllMasterDevices,sizeof(m),m};XISelectEvents(d,g_root,&mask,1);
 int w=g_width,h=g_height;PointerBarrier barrier=XFixesCreatePointerBarrier(d,g_root,20,h,w-20,h,BarrierNegativeY,0,NULL);XSync(d,False);
 XTestFakeMotionEvent(d,-1,w/2,h/2,CurrentTime);XSync(d,False);hits(d,op);
 for(int i=0;i<6;i++){XTestFakeMotionEvent(d,-1,w/2,h-1,CurrentTime);XSync(d,False);}
 int warp_hits=hits(d,op);CHECK(warp_hits==0);
 XTestFakeMotionEvent(d,-1,w/2,h/2,CurrentTime);XSync(d,False);hits(d,op);
 for(int i=0;i<6;i++){CHECK(x11_mouse_move(w/2,h-1)==0);XSync(d,False);usleep(20000);}
 int pressure_hits=hits(d,op);CHECK(pressure_hits>=5);
 XFixesDestroyPointerBarrier(d,barrier);XSync(d,False);
 // Ordinary absolute targets remain exact, including a negative displacement.
 int x,y;unsigned int state;CHECK(x11_mouse_move(100,100)==0);CHECK(x11_pointer_state(&x,&y,&state)==0);CHECK(x==100&&y==100);
 CHECK(x11_mouse_move(20,30)==0);CHECK(x11_pointer_state(&x,&y,&state)==0);CHECK(x==20&&y==30);
 CHECK(x11_mouse_move(w-1,h-1)==0);CHECK(x11_pointer_state(&x,&y,&state)==0);CHECK(x==w-1&&y==h-1);
 printf("legacy_warp_hits=%d fixed_edge_pressure_hits=%d interior_targets_exact=true edge_clamped=true result=PASS\n",warp_hits,pressure_hits);x11_close();return 0;}
