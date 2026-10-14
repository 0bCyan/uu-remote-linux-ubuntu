"""Hold an ephemeral idle inhibition only in the isolated QEMU acceptance VM."""
import pathlib,subprocess,signal,threading,sys
assert pathlib.Path('/proc/sys/kernel/hostname').read_text().strip()=='uu-hyprland-test'
assert subprocess.check_output(['systemd-detect-virt'],text=True).strip()=='qemu'
import gi
gi.require_version('Gio','2.0')
from gi.repository import Gio,GLib
seconds=min(4200,max(1,int(sys.argv[1]) if len(sys.argv)>1 else 3600))
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
reply=bus.call_sync('org.freedesktop.ScreenSaver','/ScreenSaver','org.freedesktop.ScreenSaver','Inhibit',GLib.Variant('(ss)',('UU isolated acceptance','Timed live-connection acceptance')),GLib.VariantType.new('(u)'),Gio.DBusCallFlags.NONE,3000,None)
cookie=reply.unpack()[0]
stop=threading.Event()
signal.signal(signal.SIGTERM,lambda *_:stop.set())
signal.signal(signal.SIGINT,lambda *_:stop.set())
print('VM timed idle inhibition acquired',flush=True)
try:stop.wait(seconds)
finally:
 try:
  bus.call_sync('org.freedesktop.ScreenSaver','/ScreenSaver','org.freedesktop.ScreenSaver','UnInhibit',GLib.Variant('(u)',(cookie,)),None,Gio.DBusCallFlags.NONE,3000,None)
  print('VM timed idle inhibition released',flush=True)
 except GLib.Error:print('VM idle inhibition connection closed',flush=True)
