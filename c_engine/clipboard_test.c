/* Run only in a dedicated Xvfb display; never touches the desktop clipboard. */
#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
void *clipboard_open(const char *);
int clipboard_set(void *, const unsigned char *, size_t);
int clipboard_poll(void *, unsigned char *, size_t);
void clipboard_close(void *);

int main(void) {
    XInitThreads();
    Display *d = XOpenDisplay(NULL); assert(d);
    Window w = XCreateSimpleWindow(d, DefaultRootWindow(d), 0,0,1,1,0,0,0);
    Atom selection = XInternAtom(d,"CLIPBOARD",False);
    Atom utf8 = XInternAtom(d,"UTF8_STRING",False);
    Atom property = XInternAtom(d,"TEST_CLIPBOARD",False);
    XSetSelectionOwner(d,selection,w,CurrentTime); XSync(d,False);
    void *bridge = clipboard_open(NULL); assert(bridge);
    unsigned char buffer[65536];
    assert(clipboard_poll(bridge,buffer,sizeof(buffer)) == -2);
    /* Two copies by the SAME owner must both notify the bridge. */
    for (int copy=0;copy<2;++copy) {
        const char *test = copy ? "second copy\n中文🙂" : "first copy\n中文🙂";
        XSetSelectionOwner(d,selection,w,CurrentTime); XSync(d,False);
        int result = -2;
        for (int i=0;i<300 && result<0;++i) {
            result = clipboard_poll(bridge,buffer,sizeof(buffer));
            while(XPending(d)) {
                XEvent event; XNextEvent(d,&event);
                if(event.type!=SelectionRequest) continue;
                XSelectionRequestEvent *r=&event.xselectionrequest;
                assert(r->target==utf8);
                XChangeProperty(d,r->requestor,r->property,utf8,8,PropModeReplace,
                    (const unsigned char *)test,strlen(test));
                XEvent reply; memset(&reply,0,sizeof(reply));
                reply.xselection=(XSelectionEvent){.type=SelectionNotify,.display=d,
                    .requestor=r->requestor,.selection=r->selection,.target=r->target,
                    .property=r->property,.time=r->time};
                XSendEvent(d,r->requestor,False,0,&reply); XFlush(d);
            }
            usleep(1000);
        }
        assert(result==(int)strlen(test)); assert(!memcmp(buffer,test,result));
    }
    const char *remote="remote copy\n中文🙂";
    assert(clipboard_set(bridge,(const unsigned char *)remote,strlen(remote))==0);
    XConvertSelection(d,selection,utf8,property,w,CurrentTime); XFlush(d);
    int received=0;
    for(int i=0;i<300 && !received;++i) {
        /* Owning our own remote copy must not send it back in an echo loop. */
        assert(clipboard_poll(bridge,buffer,sizeof(buffer))==-2);
        while(XPending(d)) {
            XEvent event; XNextEvent(d,&event);
            if(event.type!=SelectionNotify) continue;
            assert(event.xselection.property!=None);
            Atom actual; int format; unsigned long n,remaining; unsigned char *data;
            assert(XGetWindowProperty(d,w,property,0,65536/4,True,AnyPropertyType,
                &actual,&format,&n,&remaining,&data)==Success);
            assert(actual==utf8 && format==8 && !remaining);
            assert(n==strlen(remote) && !memcmp(data,remote,n)); XFree(data);
            received=1;
        }
        usleep(1000);
    }
    assert(received);
    assert(clipboard_set(bridge,buffer,65537)==-1);
    clipboard_close(bridge); XDestroyWindow(d,w); XCloseDisplay(d);
    puts("PASS: no initial read; same-owner copies; Unicode both directions; no echo; size bound");
}
