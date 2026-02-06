import { useCallback, useEffect, useRef, useState } from "react";

interface HotkeyInputProps {
  value: string;
  onChange: (hotkey: string) => void;
  disabled?: boolean;
  placeholder?: string;
}

/**
 * Parse a hotkey string into its modifier and key components.
 * Format: "Mod1+Mod2+Key" (e.g., "Ctrl+Shift+Space")
 */
function parseHotkey(hotkey: string): { modifiers: string[]; key: string } {
  const parts = hotkey.split("+");
  const key = parts.pop() || "";
  return { modifiers: parts, key };
}

/**
 * Format a hotkey for display (more readable version).
 */
function formatHotkeyDisplay(hotkey: string): string {
  if (!hotkey) return "";

  const { modifiers, key } = parseHotkey(hotkey);
  const displayParts: string[] = [];

  // Order modifiers consistently
  if (modifiers.includes("Ctrl") || modifiers.includes("Control")) {
    displayParts.push("Ctrl");
  }
  if (modifiers.includes("Alt")) {
    displayParts.push("Alt");
  }
  if (modifiers.includes("Shift")) {
    displayParts.push("Shift");
  }
  if (modifiers.includes("Meta") || modifiers.includes("Super") || modifiers.includes("Command")) {
    displayParts.push("Meta");
  }

  // Format the key nicely
  let displayKey = key;
  if (key === " " || key.toLowerCase() === "space") {
    displayKey = "Space";
  } else if (key.length === 1) {
    displayKey = key.toUpperCase();
  }

  displayParts.push(displayKey);
  return displayParts.join(" + ");
}

/**
 * Convert a keyboard event to a hotkey string in Tauri format.
 */
function keyboardEventToHotkey(event: KeyboardEvent): string | null {
  // Ignore modifier-only presses
  if (["Control", "Shift", "Alt", "Meta"].includes(event.key)) {
    return null;
  }

  // Ignore IME/dead key events
  if (event.key === "Dead") {
    return null;
  }

  const parts: string[] = [];

  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Meta");

  // Normalize the key
  let key = event.key;
  if (key === " ") {
    key = "Space";
  } else if (key.length === 1) {
    key = key.toUpperCase();
  } else {
    // Capitalize first letter of special keys (Enter, Tab, etc.)
    key = key.charAt(0).toUpperCase() + key.slice(1);
  }

  // If no modifiers are pressed, only allow "safe" single keys
  // (avoid capturing normal typing keys like letters/numbers/space).
  if (parts.length === 0) {
    const safeSingles = new Set([
      "Pause",
      "ScrollLock",
      "CapsLock",
      "NumLock",
      "Insert",
      "Home",
      "End",
      "PageUp",
      "PageDown",
      "Delete",
      "ArrowUp",
      "ArrowDown",
      "ArrowLeft",
      "ArrowRight",
      "PrintScreen",
      "ContextMenu",
    ]);

    const isFnKey = /^F\d{1,2}$/.test(key);
    const isSafe = isFnKey || safeSingles.has(key);
    if (!isSafe) {
      return null;
    }
  }

  parts.push(key);
  return parts.join("+");
}

/**
 * A component for capturing and displaying keyboard hotkey combinations.
 * Click to start recording, press a key combination, and it will be captured.
 */
const HotkeyInput = ({
  value,
  onChange,
  disabled = false,
  placeholder = "Press record to set hotkey",
}: HotkeyInputProps) => {
  const [isRecording, setIsRecording] = useState(false);
  const [pendingHotkey, setPendingHotkey] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const handleKeyDown = useCallback(
    (event: KeyboardEvent) => {
      if (!isRecording) return;

      event.preventDefault();
      event.stopPropagation();

      // Handle escape to cancel
      if (event.key === "Escape") {
        setIsRecording(false);
        setPendingHotkey(null);
        return;
      }

      const hotkey = keyboardEventToHotkey(event);
      if (hotkey) {
        setPendingHotkey(hotkey);
      }
    },
    [isRecording]
  );

  const handleKeyUp = useCallback(
    (event: KeyboardEvent) => {
      if (!isRecording || !pendingHotkey) return;

      event.preventDefault();
      event.stopPropagation();

      // Commit the hotkey on key release
      onChange(pendingHotkey);
      setIsRecording(false);
      setPendingHotkey(null);
    },
    [isRecording, pendingHotkey, onChange]
  );

  useEffect(() => {
    if (isRecording) {
      window.addEventListener("keydown", handleKeyDown, true);
      window.addEventListener("keyup", handleKeyUp, true);
      return () => {
        window.removeEventListener("keydown", handleKeyDown, true);
        window.removeEventListener("keyup", handleKeyUp, true);
      };
    }
  }, [isRecording, handleKeyDown, handleKeyUp]);

  // Close recording if clicking outside
  useEffect(() => {
    if (!isRecording) return;

    const handleClickOutside = (event: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setIsRecording(false);
        setPendingHotkey(null);
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [isRecording]);

  const toggleRecording = () => {
    if (disabled) return;
    if (isRecording) {
      setIsRecording(false);
      setPendingHotkey(null);
      return;
    }
    setIsRecording(true);
    setPendingHotkey(null);
  };

  const displayValue = isRecording
    ? pendingHotkey
      ? formatHotkeyDisplay(pendingHotkey)
      : "Press a key combination..."
    : value
      ? formatHotkeyDisplay(value)
      : placeholder;

  return (
    <div ref={containerRef} className="flex items-center gap-2">
      <div
        className={`min-w-[180px] flex-1 rounded-vibe border px-3 py-2 text-left text-sm transition-colors ${
          disabled
            ? "cursor-not-allowed border-border bg-surface2 text-muted"
            : isRecording
              ? "border-bad/40 bg-bad/10 text-fg ring-2 ring-bad/25"
              : "border-border bg-surface2 text-fg"
        }`}
        aria-live="polite"
      >
        <span className={!value && !isRecording ? "text-muted" : ""}>{displayValue}</span>
      </div>

      <button
        type="button"
        onClick={toggleRecording}
        disabled={disabled}
        aria-label={isRecording ? "Stop recording hotkey" : "Record hotkey"}
        title={isRecording ? "Stop recording" : "Record"}
        className={`inline-flex h-10 w-10 items-center justify-center rounded-vibe border transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-offset-2 focus-visible:ring-offset-bg ${
          disabled
            ? "cursor-not-allowed border-border bg-surface2 text-muted"
            : isRecording
              ? "border-bad/40 bg-bad/15 text-bad"
              : "border-border bg-surface2 text-muted hover:bg-surface"
        }`}
      >
        {isRecording ? (
          <svg viewBox="0 0 20 20" className="h-4 w-4" aria-hidden="true">
            <rect x="5.5" y="5.5" width="9" height="9" rx="1.5" fill="currentColor" />
          </svg>
        ) : (
          <svg viewBox="0 0 20 20" className="h-4 w-4 text-bad" aria-hidden="true">
            <circle cx="10" cy="10" r="5" fill="currentColor" />
          </svg>
        )}
      </button>
    </div>
  );
};

export default HotkeyInput;
