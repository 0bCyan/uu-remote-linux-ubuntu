"""Official viewer clipboard fixture; only synthetic-marker match booleans are recorded."""
import gi,json
from pathlib import Path
gi.require_version('Gtk','4.0'); gi.require_version('Gdk','4.0')
from gi.repository import Gtk,Gdk,GLib
assert Path('/proc/sys/kernel/hostname').read_text().strip()=='uu-hyprland-test'
state={'native_wayland':False,'mac_to_vm':False,'reverse_owned':False,'button_presses':0,'button_releases':0,'check_activations':0,'copy_activations':0,'window_active':False}
report=Path('/tmp/uuremote-clipboard-official.json')
app=Gtk.Application(application_id='dev.uuremote.ClipboardOfficial')
def activate(app):
 window=Gtk.ApplicationWindow(application=app,title='UU clipboard official acceptance')
 box=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=24)
 box.set_halign(Gtk.Align.CENTER);box.set_valign(Gtk.Align.CENTER)
 label=Gtk.Label(label='UU Portal clipboard acceptance')
 check=Gtk.Button(label='Check Mac → VM')
 copy=Gtk.Button(label='Copy VM → Mac')
 box.append(label);box.append(check);box.append(copy);window.set_child(box)
 clipboard=window.get_clipboard()
 def read(*_):
  state['check_activations']+=1
  def done(source,result):
   try: state['mac_to_vm']=source.read_text_finish(result)=='UU-Mac-to-VM 中文🙂'
   except GLib.Error: state['mac_to_vm']=False
   label.set_label('Mac → VM: '+('PASS' if state['mac_to_vm'] else 'not matched'))
  clipboard.read_text_async(None,done)
 def write(*_):
  state['copy_activations']+=1
  clipboard.set('UU-VM-to-Mac 中文🚀');state['reverse_owned']=True
  label.set_label('VM marker copied')
 check.connect('clicked',read);copy.connect('clicked',write)
 window.connect('notify::is-active',lambda *_:state.update(window_active=window.is_active()))
 gesture=Gtk.GestureClick();gesture.set_propagation_phase(Gtk.PropagationPhase.CAPTURE)
 def pressed_seen(gesture,count,x,y):
  state['button_presses']+=1
  state['last_button']=gesture.get_current_button()
  state['last_modifiers']=int(gesture.get_current_event_state())
  state['last_position']=[round(x,1),round(y,1)]
  widget=window.pick(x,y,Gtk.PickFlags.DEFAULT)
  state['last_target']='other'
  while widget:
   if widget is check:state['last_target']='check';break
   if widget is copy:state['last_target']='copy';break
   widget=widget.get_parent()
  # An observer must not compete with the button's own click gesture.
  gesture.set_state(Gtk.EventSequenceState.DENIED)
 gesture.connect('pressed',pressed_seen);window.add_controller(gesture)
 window.present();window.fullscreen()
 state['native_wayland']='wayland' in type(Gdk.Display.get_default()).__name__.lower()
 def record(): report.write_text(json.dumps(state));return True
 GLib.timeout_add(200,record)
app.connect('activate',activate);app.run([])
