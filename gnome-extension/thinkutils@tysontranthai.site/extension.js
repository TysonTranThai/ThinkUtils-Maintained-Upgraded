import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import St from 'gi://St';

import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

export default class ThinkUtilsExtension extends Extension {
  enable() {
    this._indicator = new PanelMenu.Button(0.0, this.metadata.name, false);

    // Panel display container
    this._panelBox = new St.BoxLayout({
      style_class: 'panel-status-menu-box',
      reactive: true,
      can_focus: true,
      track_hover: true,
      y_align: Clutter.ActorAlign.CENTER
    });

    // Temperature display
    this._tempLabel = new St.Label({
      text: '--°C',
      y_align: Clutter.ActorAlign.CENTER,
      style: 'margin-right: 6px;'
    });

    // Fan RPM display
    this._fanLabel = new St.Label({
      text: '-- RPM',
      y_align: Clutter.ActorAlign.CENTER
    });

    this._panelBox.add_child(this._tempLabel);
    this._panelBox.add_child(this._fanLabel);
    this._indicator.add_child(this._panelBox);

    // Build dropdown menu
    this._buildMenu();

    // Add to GNOME top panel (right side status area)
    Main.panel.addToStatusArea(this.uuid, this._indicator, 1, 'right');

    // Initial metrics update and recurring 2-second timer
    this._updateMetrics();
    this._timeoutId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 2, () => {
      this._updateMetrics();
      return GLib.SOURCE_CONTINUE;
    });
  }

  _buildMenu() {
    const menu = this._indicator.menu;

    // Title item
    const titleItem = new PopupMenu.PopupMenuItem('ThinkUtils Hardware Monitor', {
      reactive: false,
      can_focus: false,
      style_class: 'popup-subtitle-menu-item'
    });
    menu.addMenuItem(titleItem);
    menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

    // Status items
    this._menuTempItem = new PopupMenu.PopupMenuItem('CPU Temperature: --°C', { reactive: false });
    menu.addMenuItem(this._menuTempItem);

    this._menuFanItem = new PopupMenu.PopupMenuItem('Fan: -- RPM (Mode: --)', { reactive: false });
    menu.addMenuItem(this._menuFanItem);

    this._menuBatteryItem = new PopupMenu.PopupMenuItem('Battery: --%', { reactive: false });
    menu.addMenuItem(this._menuBatteryItem);

    menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

    // Fan control quick actions
    const fanHeader = new PopupMenu.PopupMenuItem('Quick Fan Controls', {
      reactive: false,
      can_focus: false
    });
    menu.addMenuItem(fanHeader);

    const autoItem = new PopupMenu.PopupMenuItem('  Auto Mode');
    autoItem.connect('activate', () => {
      this._setFanSpeed('level auto');
    });
    menu.addMenuItem(autoItem);

    const mediumItem = new PopupMenu.PopupMenuItem('  Level 4 (Medium)');
    mediumItem.connect('activate', () => {
      this._setFanSpeed('level 4');
    });
    menu.addMenuItem(mediumItem);

    const highItem = new PopupMenu.PopupMenuItem('  Level 7 (High)');
    highItem.connect('activate', () => {
      this._setFanSpeed('level 7');
    });
    menu.addMenuItem(highItem);

    const fullSpeedItem = new PopupMenu.PopupMenuItem('  Full Speed (Maximum)');
    fullSpeedItem.connect('activate', () => {
      this._setFanSpeed('level full-speed');
    });
    menu.addMenuItem(fullSpeedItem);

    menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

    // Launch ThinkUtils app
    const openAppItem = new PopupMenu.PopupMenuItem('Open ThinkUtils');
    openAppItem.connect('activate', () => {
      this._launchApp();
    });
    menu.addMenuItem(openAppItem);
  }

  _readCpuTemp() {
    try {
      const [ok, data] = GLib.file_get_contents('/proc/acpi/ibm/thermal');
      if (ok) {
        const str = new TextDecoder().decode(data);
        const match = str.match(/temperatures:\s+(-?\d+)/);
        if (match) {
          const t = parseInt(match[1], 10);
          if (t > 0 && t < 130) {
            return t;
          }
        }
      }
    } catch (_err) {
      // Thermal fallback
    }

    try {
      const [ok, data] = GLib.file_get_contents('/sys/class/thermal/thermal_zone0/temp');
      if (ok) {
        const t = parseInt(new TextDecoder().decode(data).trim(), 10);
        if (!isNaN(t) && t > 0) {
          return Math.round(t / 1000);
        }
      }
    } catch (_err) {
      // Fallback unavailable
    }

    return null;
  }

  _readFanInfo() {
    try {
      const [ok, data] = GLib.file_get_contents('/proc/acpi/ibm/fan');
      if (ok) {
        const str = new TextDecoder().decode(data);
        const speedMatch = str.match(/speed:\s+(\d+)/);
        const levelMatch = str.match(/level:\s+(\S+)/);
        const statusMatch = str.match(/status:\s+(\S+)/);
        return {
          speed: speedMatch ? parseInt(speedMatch[1], 10) : 0,
          level: levelMatch ? levelMatch[1] : 'unknown',
          status: statusMatch ? statusMatch[1] : 'unknown'
        };
      }
    } catch (_err) {
      // Fan procfs unreadable
    }
    return null;
  }

  _readBatteryInfo() {
    try {
      const [okCap, capData] = GLib.file_get_contents('/sys/class/power_supply/BAT0/capacity');
      const [okStat, statData] = GLib.file_get_contents('/sys/class/power_supply/BAT0/status');
      const [okAc, acData] = GLib.file_get_contents('/sys/class/power_supply/AC/online');
      if (okCap && okStat) {
        const isAc = okAc ? new TextDecoder().decode(acData).trim() === '1' : false;
        return {
          capacity: parseInt(new TextDecoder().decode(capData).trim(), 10),
          status: new TextDecoder().decode(statData).trim(),
          isAc
        };
      }
    } catch (_err) {
      // Battery sysfs unreadable
    }
    return null;
  }

  _updateMetrics() {
    const temp = this._readCpuTemp();
    const fan = this._readFanInfo();
    const battery = this._readBatteryInfo();

    if (this._tempLabel) {
      if (temp !== null) {
        this._tempLabel.text = `${temp}°C`;
        if (temp >= 85) {
          this._tempLabel.style = 'color: #ef4444; font-weight: bold; margin-right: 6px;';
        } else if (temp >= 70) {
          this._tempLabel.style = 'color: #f59e0b; font-weight: bold; margin-right: 6px;';
        } else {
          this._tempLabel.style = 'margin-right: 6px;';
        }
      } else {
        this._tempLabel.text = '--°C';
      }
    }

    if (this._fanLabel) {
      if (fan) {
        this._fanLabel.text = `${fan.speed} RPM`;
      } else {
        this._fanLabel.text = '-- RPM';
      }
    }

    if (this._menuTempItem) {
      this._menuTempItem.label.text =
        temp !== null ? `CPU Temperature: ${temp}°C` : 'CPU Temperature: Unknown';
    }

    if (this._menuFanItem) {
      if (fan) {
        this._menuFanItem.label.text = `Fan: ${fan.speed} RPM (Mode: ${fan.level})`;
      } else {
        this._menuFanItem.label.text = 'Fan: Unknown';
      }
    }

    if (this._menuBatteryItem) {
      if (battery) {
        const acStr = battery.isAc ? ' (AC Connected)' : '';
        this._menuBatteryItem.label.text = `Battery: ${battery.capacity}% - ${battery.status}${acStr}`;
      } else {
        this._menuBatteryItem.label.text = 'Battery: Not detected';
      }
    }
  }

  _setFanSpeed(command) {
    try {
      const helper = '/usr/local/bin/thinkutils-fan-control';
      if (GLib.file_test(helper, GLib.FileTest.EXISTS)) {
        GLib.spawn_command_line_async(`pkexec ${helper} "${command}"`);
      } else {
        GLib.spawn_command_line_async(`pkexec bash -c 'echo "${command}" > /proc/acpi/ibm/fan'`);
      }
      GLib.timeout_add(GLib.PRIORITY_DEFAULT, 500, () => {
        this._updateMetrics();
        return GLib.SOURCE_REMOVE;
      });
    } catch (e) {
      console.error('[ThinkUtils Extension] Error setting fan speed:', e);
    }
  }

  _launchApp() {
    try {
      const [ok, out] = GLib.spawn_command_line_sync('pgrep -x thinkutils');
      if (ok && out && out.length > 0) {
        GLib.spawn_command_line_async('thinkutils');
      } else {
        try {
          const appInfo = Gio.DesktopAppInfo.new('thinkutils.desktop');
          if (appInfo) {
            appInfo.launch([], null);
            return;
          }
        } catch (_err) {
          // Desktop app fallback
        }
        GLib.spawn_command_line_async('thinkutils');
      }
    } catch (e) {
      console.error('[ThinkUtils Extension] Error launching app:', e);
    }
  }

  disable() {
    if (this._timeoutId) {
      GLib.Source.remove(this._timeoutId);
      this._timeoutId = null;
    }

    if (this._indicator) {
      this._indicator.destroy();
      this._indicator = null;
    }

    this._panelBox = null;
    this._tempLabel = null;
    this._fanLabel = null;
    this._menuTempItem = null;
    this._menuFanItem = null;
    this._menuBatteryItem = null;
  }
}
