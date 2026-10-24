// Ubuntu fork regression fixture by 0bCyan, 2026-10-24; see NOTICE.md.
#include <X11/Xlib.h>
#include <time.h>
#include <stdlib.h>
#include <string.h>
int main(){if(!getenv("DISPLAY")||strcmp(getenv("DISPLAY"),":98"))return 2;Display*d=XOpenDisplay(NULL);if(!d)return 3;Window w=DefaultRootWindow(d);GC g=XCreateGC(d,w,0,NULL);struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);for(unsigned n=0;;n++){XSetForeground(d,g,n&1?0x607fa0:0x809fc0);XFillRectangle(d,w,g,0,0,3120,2080);XSetForeground(d,g,0xff8040);XFillRectangle(d,w,g,(n*13)%2700,400,384,384);XSync(d,False);t.tv_nsec+=16666667;if(t.tv_nsec>=1000000000){t.tv_nsec-=1000000000;t.tv_sec++;}clock_nanosleep(CLOCK_MONOTONIC,TIMER_ABSTIME,&t,NULL);}return 0;}
