// Re-Open window list: a read-only D-Bus view of normal application windows.
//
// Exposes application ID (the .desktop file ID GNOME associates with the
// window), process ID, a window ID that is stable for the window's lifetime,
// workspace index and minimized state. Window titles and contents are never
// exposed (docs/SECURITY.md). Anything on the user's session bus can call it;
// that is the same-user boundary the agent already relies on.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

const BUS_NAME = 'com.caffcore.ReOpen.Windows';
const OBJECT_PATH = '/com/caffcore/ReOpen/Windows';
const INTERFACE_XML = `
<node>
  <interface name="com.caffcore.ReOpen.Windows1">
    <method name="GetWindows">
      <arg type="aa{sv}" direction="out" name="windows"/>
    </method>
    <property name="Version" type="u" access="read"/>
  </interface>
</node>`;

/** Bumped when the shape of GetWindows changes. */
const API_VERSION = 1;

export default class ReOpenWindowsExtension extends Extension {
  enable() {
    this._service = Gio.DBusExportedObject.wrapJSObject(INTERFACE_XML, this);
    this._service.export(Gio.DBus.session, OBJECT_PATH);
    this._nameId = Gio.bus_own_name(
      Gio.BusType.SESSION,
      BUS_NAME,
      Gio.BusNameOwnerFlags.NONE,
      null,
      null,
      null,
    );
  }

  disable() {
    if (this._nameId) Gio.bus_unown_name(this._nameId);
    this._nameId = 0;
    this._service?.unexport();
    this._service = null;
  }

  get Version() {
    return API_VERSION;
  }

  GetWindows() {
    const tracker = Shell.WindowTracker.get_default();
    return global
      .get_window_actors()
      .map((actor) => actor.meta_window)
      .filter((window) => window.get_window_type() === Meta.WindowType.NORMAL)
      .filter((window) => !window.is_skip_taskbar())
      .map((window) => {
        const app = tracker.get_window_app(window);
        const workspace = window.get_workspace();
        return {
          app_id: new GLib.Variant('s', app?.get_id() ?? ''),
          pid: new GLib.Variant('i', window.get_pid()),
          window_id: new GLib.Variant('t', window.get_id()),
          workspace: new GLib.Variant('i', workspace ? workspace.index() : -1),
          minimized: new GLib.Variant('b', window.minimized),
        };
      });
  }
}
