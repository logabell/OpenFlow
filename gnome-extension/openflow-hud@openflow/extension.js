import Cairo from "cairo";
import Gio from "gi://Gio";
import GLib from "gi://GLib";
import St from "gi://St";
import * as Main from "resource:///org/gnome/shell/ui/main.js";
import { Extension } from "resource:///org/gnome/shell/extensions/extension.js";

const HUD_SIZE = 104;
const HUD_MARGIN_BOTTOM = 62;
const ORB_RADIUS = 42;
const HALO_SIZE = ORB_RADIUS * 2;
const HALO_OFFSET = Math.floor((HUD_SIZE - HALO_SIZE) / 2);
const POLL_INTERVAL_MS = 120;
const ANIMATION_INTERVAL_MS = 16;
const EXIT_HIDE_DELAY_MS = 260;
const STARTUP_WRITE_GRACE_MS = 700;
const TAU = Math.PI * 2;

const STATE_COLORS = {
    listening: {
        halo: "rgba(32, 178, 255, 0.42)",
        ring: "rgba(118, 216, 255, 0.68)",
        arc: "rgba(120, 230, 255, 0.95)",
        arcSoft: "rgba(112, 198, 236, 0.52)",
        spark: "rgba(194, 244, 255, 0.8)",
    },
    processing: {
        halo: "rgba(255, 145, 38, 0.45)",
        ring: "rgba(255, 188, 108, 0.66)",
        arc: "rgba(255, 214, 138, 0.95)",
        arcSoft: "rgba(255, 178, 96, 0.52)",
        spark: "rgba(255, 232, 180, 0.8)",
    },
    warming: {
        halo: "rgba(166, 183, 199, 0.34)",
        ring: "rgba(206, 218, 230, 0.62)",
        arc: "rgba(228, 238, 250, 0.9)",
        arcSoft: "rgba(183, 198, 214, 0.48)",
        spark: "rgba(240, 245, 250, 0.76)",
    },
    "performance-warning": {
        halo: "rgba(255, 178, 66, 0.4)",
        ring: "rgba(255, 216, 148, 0.66)",
        arc: "rgba(255, 230, 172, 0.94)",
        arcSoft: "rgba(255, 194, 110, 0.52)",
        spark: "rgba(255, 240, 198, 0.8)",
    },
    "secure-blocked": {
        halo: "rgba(255, 94, 94, 0.4)",
        ring: "rgba(255, 168, 168, 0.66)",
        arc: "rgba(255, 196, 196, 0.94)",
        arcSoft: "rgba(236, 120, 128, 0.52)",
        spark: "rgba(255, 214, 214, 0.8)",
    },
    "asr-error": {
        halo: "rgba(255, 94, 94, 0.4)",
        ring: "rgba(255, 168, 168, 0.66)",
        arc: "rgba(255, 196, 196, 0.94)",
        arcSoft: "rgba(236, 120, 128, 0.52)",
        spark: "rgba(255, 214, 214, 0.8)",
    },
};

const DEFAULT_COLORS = STATE_COLORS.warming;

function colorToRgba(color, alphaScale = 1) {
    const match = /rgba?\(([^)]+)\)/.exec(color);
    if (!match) {
        return [1, 1, 1, alphaScale];
    }

    const parts = match[1]
        .split(",")
        .map((part) => Number.parseFloat(part.trim()))
        .filter((part) => Number.isFinite(part));

    if (parts.length < 3) {
        return [1, 1, 1, alphaScale];
    }

    const red = parts[0] / 255;
    const green = parts[1] / 255;
    const blue = parts[2] / 255;
    const alpha = Math.max(0, Math.min(1, (parts[3] ?? 1) * alphaScale));
    return [red, green, blue, alpha];
}

function setColor(cr, color, alphaScale = 1) {
    const [red, green, blue, alpha] = colorToRgba(color, alphaScale);
    cr.setSourceRGBA(red, green, blue, alpha);
}

function strokeCircle(cr, cx, cy, radius, width, color, alphaScale = 1, dash = null, offset = 0) {
    cr.save();
    cr.setLineCap(Cairo.LineCap.ROUND);
    cr.setLineWidth(width);
    if (dash) {
        cr.setDash(dash, offset);
    }
    setColor(cr, color, alphaScale);
    cr.arc(cx, cy, radius, 0, TAU);
    cr.stroke();
    cr.restore();
}

function strokeEllipse(cr, options) {
    const {
        cx,
        cy,
        rx,
        ry,
        width,
        color,
        alphaScale = 1,
        rotation = 0,
        dash = null,
        offset = 0,
    } = options;

    const base = (rx + ry) * 0.5;

    cr.save();
    cr.translate(cx, cy);
    cr.rotate(rotation);
    cr.scale(rx, ry);
    cr.setLineCap(Cairo.LineCap.ROUND);
    cr.setLineWidth(width / base);
    if (dash) {
        cr.setDash(dash.map((value) => value / base), offset / base);
    }
    setColor(cr, color, alphaScale);
    cr.arc(0, 0, 1, 0, TAU);
    cr.stroke();
    cr.restore();
}

export default class OpenFlowHudExtension extends Extension {
    enable() {
        this._decoder = new TextDecoder("utf-8");
        this._lastSignature = null;
        this._state = "idle";
        this._phase = 0;
        this._driftX = 0;
        this._driftY = 0;
        this._phaseOffsetA = Math.random() * TAU;
        this._phaseOffsetB = Math.random() * TAU;
        this._colors = DEFAULT_COLORS;
        this._hideTimeoutId = null;
        this._enabledAtMicros = GLib.get_real_time();
        this._hasSeenPostEnableWrite = false;
        this._lastMonitorIndex = null;
        this._displayFocusChangedId = null;
        this._workspaceChangedId = null;

        this._container = new St.Widget({
            reactive: false,
            can_focus: false,
            track_hover: false,
            visible: false,
            width: HUD_SIZE,
            height: HUD_SIZE,
        });

        this._halo = new St.Widget({
            reactive: false,
            can_focus: false,
            track_hover: false,
            x: HALO_OFFSET,
            y: HALO_OFFSET,
            width: HALO_SIZE,
            height: HALO_SIZE,
            style: "border-radius: 999px; background-color: rgba(160, 180, 200, 0.35);",
        });

        this._drawingArea = new St.DrawingArea({
            reactive: false,
            can_focus: false,
            track_hover: false,
            x: 0,
            y: 0,
            width: HUD_SIZE,
            height: HUD_SIZE,
        });
        this._repaintId = this._drawingArea.connect("repaint", (area) => {
            this._drawOrb(area);
        });

        this._container.add_child(this._halo);
        this._container.add_child(this._drawingArea);
        Main.layoutManager.addChrome(this._container, { trackFullscreen: true });

        this._applyStateVisual("warming");

        this._monitorsChangedId = Main.layoutManager.connect("monitors-changed", () => {
            this._syncPosition();
        });

        if (global.display?.connect) {
            this._displayFocusChangedId = global.display.connect("notify::focus-window", () => {
                this._syncPosition();
            });
        }

        if (global.workspace_manager?.connect) {
            this._workspaceChangedId = global.workspace_manager.connect(
                "active-workspace-changed",
                () => {
                    this._syncPosition();
                }
            );
        }

        this._syncPosition();
        this._pollId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, POLL_INTERVAL_MS, () => {
            this._refresh();
            return GLib.SOURCE_CONTINUE;
        });

        this._animationId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, ANIMATION_INTERVAL_MS, () => {
            this._tickAnimation();
            return GLib.SOURCE_CONTINUE;
        });

        this._refresh();
    }

    disable() {
        if (this._hideTimeoutId) {
            GLib.Source.remove(this._hideTimeoutId);
            this._hideTimeoutId = null;
        }

        if (this._pollId) {
            GLib.Source.remove(this._pollId);
            this._pollId = null;
        }

        if (this._animationId) {
            GLib.Source.remove(this._animationId);
            this._animationId = null;
        }

        if (this._monitorsChangedId) {
            Main.layoutManager.disconnect(this._monitorsChangedId);
            this._monitorsChangedId = null;
        }

        if (this._displayFocusChangedId && global.display?.disconnect) {
            global.display.disconnect(this._displayFocusChangedId);
            this._displayFocusChangedId = null;
        }

        if (this._workspaceChangedId && global.workspace_manager?.disconnect) {
            global.workspace_manager.disconnect(this._workspaceChangedId);
            this._workspaceChangedId = null;
        }

        if (this._drawingArea && this._repaintId) {
            this._drawingArea.disconnect(this._repaintId);
            this._repaintId = null;
        }

        this._lastSignature = null;

        if (this._container) {
            this._container.destroy();
            this._container = null;
        }

        this._halo = null;
        this._drawingArea = null;
        this._decoder = null;
        this._colors = DEFAULT_COLORS;
        this._state = "idle";
        this._phase = 0;
        this._lastMonitorIndex = null;
    }

    _refresh() {
        const path = this._statePath();
        if (!path) {
            this._hide();
            return;
        }

        try {
            const [ok, bytes] = GLib.file_get_contents(path);
            if (!ok) {
                this._hide();
                return;
            }

            const payload = JSON.parse(this._decoder.decode(bytes));
            const enabled = payload?.enabled === true;
            const state = typeof payload?.state === "string" ? payload.state : "idle";
            const pid = Number.isInteger(payload?.pid) ? payload.pid : null;
            const sessionId = typeof payload?.session_id === "string" ? payload.session_id : null;
            const modifiedMicros = this._readStateModifiedMicros(path);

            if (
                !this._hasSeenPostEnableWrite &&
                modifiedMicros !== null &&
                modifiedMicros + STARTUP_WRITE_GRACE_MS * 1000 >= this._enabledAtMicros
            ) {
                this._hasSeenPostEnableWrite = true;
            }

            if (!enabled || state === "idle") {
                this._scheduleHide();
                return;
            }

            if (!this._hasSeenPostEnableWrite) {
                this._cancelHideSchedule();
                this._hide();
                return;
            }

            const signature = `${enabled ? "1" : "0"}:${state}:${pid ?? "none"}:${sessionId ?? "none"}`;
            if (signature === this._lastSignature) {
                if (this._container?.visible) {
                    this._syncPosition();
                }
                return;
            }
            this._lastSignature = signature;

            this._cancelHideSchedule();
            this._state = state;
            this._applyStateVisual(state);
            this._syncPosition();
            this._container.show();
            this._drawingArea.queue_repaint();
        } catch (_error) {
            this._scheduleHide();
        }
    }

    _readStateModifiedMicros(path) {
        try {
            const file = Gio.File.new_for_path(path);
            const info = file.query_info(
                "time::modified,time::modified-usec",
                Gio.FileQueryInfoFlags.NONE,
                null
            );

            const modifiedSec = info.get_attribute_uint64("time::modified");
            const modifiedUsec = info.get_attribute_uint32("time::modified-usec");
            return modifiedSec * 1000000 + modifiedUsec;
        } catch (_error) {
            return null;
        }
    }

    _hide() {
        if (this._container) {
            this._container.hide();
        }
        this._state = "idle";
    }

    _scheduleHide() {
        if (this._hideTimeoutId) {
            return;
        }
        this._hideTimeoutId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, EXIT_HIDE_DELAY_MS, () => {
            this._hideTimeoutId = null;
            this._hide();
            return GLib.SOURCE_REMOVE;
        });
    }

    _cancelHideSchedule() {
        if (!this._hideTimeoutId) {
            return;
        }
        GLib.Source.remove(this._hideTimeoutId);
        this._hideTimeoutId = null;
    }

    _applyStateVisual(state) {
        this._colors = STATE_COLORS[state] ?? DEFAULT_COLORS;
        this._halo.set_style(`border-radius: 999px; background-color: ${this._colors.halo};`);
    }

    _tickAnimation() {
        if (!this._container || !this._drawingArea || !this._halo || !this._container.visible) {
            return;
        }

        const isProcessing = this._state === "processing";
        this._phase += isProcessing ? 0.064 : 0.046;
        if (this._phase > TAU) {
            this._phase -= TAU;
        }

        this._driftX = Math.sin(this._phase * 0.74 + this._phaseOffsetA) * (isProcessing ? 1.1 : 0.75);
        this._driftY = Math.cos(this._phase * 0.56 + this._phaseOffsetB) * (isProcessing ? 1.3 : 0.9);

        const pulse = (Math.sin(this._phase * (isProcessing ? 1.45 : 1.05)) + 1) * 0.5;
        this._halo.opacity = Math.round(22 + pulse * (isProcessing ? 40 : 28));
        this._drawingArea.queue_repaint();
    }

    _drawOrb(area) {
        const cr = area.get_context();
        const [width, height] = area.get_surface_size();

        cr.setOperator(Cairo.Operator.CLEAR);
        cr.paint();
        cr.setOperator(Cairo.Operator.OVER);

        const isProcessing = this._state === "processing";
        const speed = isProcessing ? 1 : 1.48;
        const cx = width * 0.5 + this._driftX * 0.28;
        const cy = height * 0.5 + this._driftY * 0.28;
        const colors = this._colors ?? DEFAULT_COLORS;
        const phase = this._phase;

        strokeCircle(cr, cx, cy, ORB_RADIUS + 0.2, 1.05, colors.ring, 0.88);
        strokeCircle(cr, cx, cy, ORB_RADIUS - 2.4, 0.86, colors.arcSoft, 0.22);

        strokeCircle(
            cr,
            cx,
            cy,
            ORB_RADIUS,
            1.38,
            colors.arc,
            0.8,
            [54, 32, 18, 68, 24, 82],
            -phase * (isProcessing ? 54 : 34)
        );
        strokeCircle(
            cr,
            cx,
            cy,
            ORB_RADIUS - 0.7,
            0.92,
            colors.spark,
            0.54,
            [12, 12, 8, 24, 6, 34, 9, 22],
            phase * (isProcessing ? 46 : 28)
        );

        cr.save();
        cr.arc(cx, cy, ORB_RADIUS - 0.2, 0, TAU);
        cr.clip();

        strokeEllipse(cr, {
            cx,
            cy,
            rx: 40,
            ry: 10,
            width: 0.86,
            color: colors.arcSoft,
            alphaScale: 0.62,
            rotation: phase * (isProcessing ? 0.52 : 0.34),
            dash: [92, 168],
            offset: -phase * (isProcessing ? 60 : 38),
        });
        strokeEllipse(cr, {
            cx,
            cy,
            rx: 35,
            ry: 16,
            width: 0.72,
            color: colors.spark,
            alphaScale: 0.44,
            rotation: -phase * (isProcessing ? 0.74 : 0.46),
            dash: [66, 138],
            offset: phase * (isProcessing ? 52 : 34),
        });
        strokeEllipse(cr, {
            cx,
            cy,
            rx: 40,
            ry: 7.5,
            width: 0.64,
            color: colors.arcSoft,
            alphaScale: 0.36,
            rotation: phase * (isProcessing ? 0.15 : 0.1),
            dash: [52, 182],
            offset: -phase * (isProcessing ? 44 : 28),
        });

        cr.restore();
        cr.$dispose();
    }

    _statePath() {
        const runtimeDir = GLib.getenv("XDG_RUNTIME_DIR");
        if (!runtimeDir) {
            return null;
        }
        return GLib.build_filenamev([runtimeDir, "openflow", "hud-state.json"]);
    }

    _syncPosition() {
        if (!this._container) {
            return;
        }

        const monitors = Main.layoutManager.monitors ?? [];
        const monitorFromIndex = (index) => {
            if (!Number.isInteger(index) || index < 0) {
                return null;
            }
            return monitors[index] ?? null;
        };

        let monitor = null;
        const focusedWindow = global.display?.get_focus_window?.();
        if (focusedWindow) {
            const activeWorkspace = global.workspace_manager?.get_active_workspace?.();
            const focusedWorkspace = focusedWindow.get_workspace?.();
            const isFocusedWorkspaceActive =
                !activeWorkspace || !focusedWorkspace || focusedWorkspace === activeWorkspace;
            if (isFocusedWorkspaceActive) {
                const index = focusedWindow.get_monitor();
                monitor = monitorFromIndex(index);
                if (monitor) {
                    this._lastMonitorIndex = index;
                }
            }
        }

        if (!monitor) {
            const pointerMonitor = global.display?.get_current_monitor?.();
            monitor = monitorFromIndex(pointerMonitor);
            if (monitor) {
                this._lastMonitorIndex = pointerMonitor;
            }
        }

        if (!monitor) {
            monitor = monitorFromIndex(this._lastMonitorIndex);
        }

        if (!monitor) {
            monitor = Main.layoutManager.primaryMonitor;
        }

        if (!monitor) {
            return;
        }

        const selectedIndex = monitors.indexOf(monitor);
        if (selectedIndex >= 0) {
            this._lastMonitorIndex = selectedIndex;
        }

        const x = monitor.x + Math.floor((monitor.width - HUD_SIZE) / 2);
        const y = monitor.y + monitor.height - HUD_SIZE - HUD_MARGIN_BOTTOM;
        this._container.set_size(HUD_SIZE, HUD_SIZE);
        this._container.set_position(x, y);
    }
}
