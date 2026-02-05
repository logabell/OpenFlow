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

  const parts: string[] = [];

  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Meta");

  // Require at least one modifier for a valid hotkey
  if (parts.length === 0) {
    return null;
  }

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
  placeholder = "Click to set hotkey",
}: HotkeyInputProps) => {
  const [isRecording, setIsRecording] = useState(false);
  const [pendingHotkey, setPendingHotkey] = useState<string | null>(null);
  const inputRef = useRef<HTMLButtonElement>(null);

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
      if (inputRef.current && !inputRef.current.contains(event.target as Node)) {
        setIsRecording(false);
        setPendingHotkey(null);
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [isRecording]);

  const handleClick = () => {
    if (disabled) return;
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
    <button
      ref={inputRef}
      type="button"
      onClick={handleClick}
      disabled={disabled}
      className={`min-w-[180px] rounded-md px-3 py-2 text-left transition-all ${
        disabled
          ? "cursor-not-allowed bg-slate-800 text-slate-500"
          : isRecording
            ? "bg-cyan-900/50 text-cyan-300 ring-2 ring-cyan-500"
            : "bg-slate-900 text-white hover:bg-slate-800"
      }`}
    >
      <span className={!value && !isRecording ? "text-slate-500" : ""}>
        {displayValue}
      </span>
    </button>
  );
};

export default HotkeyInput;
