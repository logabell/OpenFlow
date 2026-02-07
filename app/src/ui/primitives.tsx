import { useId, useMemo, useState, type ReactNode } from "react";
import clsx from "clsx";

function cn(...values: Array<string | undefined | null | false>) {
  return clsx(values);
}

export type ButtonVariant = "primary" | "secondary" | "ghost";
export type ButtonSize = "sm" | "md" | "lg";

export function Button({
  variant = "secondary",
  size = "md",
  className,
  type,
  ...props
}: {
  variant?: ButtonVariant;
  size?: ButtonSize;
} & Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "type"> & {
    type?: React.ButtonHTMLAttributes<HTMLButtonElement>["type"];
  }) {
  const sizeClass =
    size === "sm"
      ? "h-8 px-3 text-xs"
      : size === "lg"
        ? "h-11 px-5 text-sm"
        : "h-10 px-4 text-sm";

  const variantClass =
    variant === "primary"
      ? "border-accent/60 bg-accent text-bg shadow-[0_2px_0_hsl(var(--shadow)/0.22)] hover:bg-accent/90"
      : variant === "ghost"
        ? "border-transparent bg-transparent text-fg hover:border-border hover:bg-surface2"
        : "border-border bg-surface2 text-fg shadow-[0_2px_0_hsl(var(--shadow)/0.16)] hover:bg-surface";

  return (
    <button
      type={type ?? "button"}
      className={cn(
        "inline-flex items-center justify-center gap-2 rounded-vibe border font-medium transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/35",
        "disabled:cursor-not-allowed disabled:opacity-50",
        sizeClass,
        variantClass,
        className,
      )}
      {...props}
    />
  );
}

export function Card({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        "rounded-vibe border border-border bg-surface shadow-[0_2px_0_hsl(var(--shadow)/0.18)]",
        className,
      )}
      {...props}
    />
  );
}

export type BadgeTone = "neutral" | "good" | "warn" | "bad" | "info";

export function Badge({
  tone = "neutral",
  className,
  ...props
}: {
  tone?: BadgeTone;
} & React.HTMLAttributes<HTMLSpanElement>) {
  const toneClass =
    tone === "good"
      ? "border-good/30 bg-good/10 text-good"
      : tone === "warn"
        ? "border-warn/30 bg-warn/10 text-warn"
        : tone === "bad"
          ? "border-bad/30 bg-bad/10 text-bad"
          : tone === "info"
            ? "border-info/30 bg-info/10 text-info"
            : "border-border bg-surface2 text-muted";

  return (
    <span
      className={cn(
        "inline-flex items-center rounded-vibe border px-2 py-0.5 text-xs font-semibold",
        toneClass,
        className,
      )}
      {...props}
    />
  );
}

export function Kbd({ className, ...props }: React.HTMLAttributes<HTMLElement>) {
  return (
    <kbd
      className={cn(
        "inline-flex items-center rounded-vibe border border-border bg-surface2 px-2 py-0.5",
        "font-mono text-xs font-semibold text-fg shadow-[0_1px_0_hsl(var(--shadow)/0.18)]",
        className,
      )}
      {...props}
    />
  );
}

export type SelectWidth = "sm" | "md" | "lg" | "full";
export type SelectSize = "sm" | "md";

export type SelectOption<T extends string> = {
  value: T;
  label: string;
  description?: string;
  disabled?: boolean;
};

export function Select<T extends string>({
  value,
  onChange,
  options,
  width = "md",
  size = "md",
  ariaLabel,
  className,
  ...props
}: {
  value: T;
  onChange: (value: T) => void;
  options: Array<SelectOption<T>>;
  width?: SelectWidth;
  size?: SelectSize;
  ariaLabel?: string;
} & Omit<React.SelectHTMLAttributes<HTMLSelectElement>, "value" | "onChange" | "size">) {
  const widthClass =
    width === "full"
      ? "w-full"
      : width === "lg"
        ? "w-72"
        : width === "sm"
          ? "w-40"
          : "w-56";

  const sizeClass = size === "sm" ? "h-8 px-2 text-xs" : "h-10 px-3 text-sm";

  return (
    <select
      value={value}
      onChange={(event) => onChange(event.target.value as T)}
      aria-label={ariaLabel}
      className={cn(
        "rounded-vibe border border-border bg-surface2 text-fg shadow-[0_2px_0_hsl(var(--shadow)/0.16)]",
        "outline-none focus:border-accent/50 focus:ring-2 focus:ring-accent/20",
        widthClass,
        sizeClass,
        className,
      )}
      {...props}
    >
      {options.map((opt) => (
        <option key={opt.value} value={opt.value} disabled={opt.disabled}>
          {opt.description ? `${opt.label} — ${opt.description}` : opt.label}
        </option>
      ))}
    </select>
  );
}

export function AccordionSection({
  title,
  description,
  open,
  onToggle,
  children,
}: {
  title: string;
  description?: string;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  const contentId = useId();

  return (
    <div className="rounded-vibe border border-border bg-surface">
      <button
        type="button"
        className={cn(
          "flex w-full items-start justify-between gap-3 px-4 py-3 text-left",
          "hover:bg-surface2",
        )}
        onClick={onToggle}
        aria-expanded={open}
        aria-controls={contentId}
      >
        <div>
          <div className="text-sm font-semibold text-fg">{title}</div>
          {description && <div className="mt-0.5 text-xs text-muted">{description}</div>}
        </div>
        <span
          className={cn(
            "mt-0.5 inline-flex h-6 w-6 items-center justify-center rounded-vibe border border-border bg-surface2 text-muted",
            open ? "rotate-180" : "rotate-0",
            "transition-transform",
          )}
          aria-hidden="true"
        >
          <svg viewBox="0 0 20 20" className="h-4 w-4" fill="none" stroke="currentColor">
            <path
              d="M5 7.5L10 12.5L15 7.5"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </span>
      </button>

      {open && <div id={contentId} className="border-t border-border px-4 py-3">{children}</div>}
    </div>
  );
}

export function Disclosure({
  title,
  description,
  defaultOpen = false,
  children,
}: {
  title: string;
  description?: string;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const contentId = useId();

  return (
    <div className="rounded-vibe border border-border bg-surface">
      <button
        type="button"
        className={cn(
          "flex w-full items-start justify-between gap-3 px-4 py-3 text-left",
          "hover:bg-surface2",
        )}
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        aria-controls={contentId}
      >
        <div>
          <div className="text-sm font-semibold text-fg">{title}</div>
          {description && <div className="mt-0.5 text-xs text-muted">{description}</div>}
        </div>
        <span
          className={cn(
            "mt-0.5 inline-flex h-6 w-6 items-center justify-center rounded-vibe border border-border bg-surface2 text-muted",
            open ? "rotate-180" : "rotate-0",
            "transition-transform",
          )}
          aria-hidden="true"
        >
          <svg viewBox="0 0 20 20" className="h-4 w-4" fill="none" stroke="currentColor">
            <path
              d="M5 7.5L10 12.5L15 7.5"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </span>
      </button>

      {open && <div id={contentId} className="border-t border-border px-4 py-3">{children}</div>}
    </div>
  );
}

export type TabsItem<T extends string> = {
  value: T;
  label: string;
  disabled?: boolean;
};

export function Tabs<T extends string>({
  value,
  onChange,
  tabs,
}: {
  value: T;
  onChange: (value: T) => void;
  tabs: Array<TabsItem<T>>;
}) {
  const rendered = useMemo(() => tabs, [tabs]);
  return (
    <div className="inline-flex items-center rounded-vibe border border-border bg-surface2 p-0.5">
      {rendered.map((tab) => {
        const active = tab.value === value;
        return (
          <button
            key={tab.value}
            type="button"
            className={cn(
              "rounded-vibe px-3 py-1 text-xs font-semibold transition-colors",
              tab.disabled ? "cursor-not-allowed opacity-50" : "hover:bg-surface",
              active
                ? "bg-surface text-fg shadow-[0_1px_0_hsl(var(--shadow)/0.18)]"
                : "text-muted",
            )}
            disabled={tab.disabled}
            onClick={() => {
              if (!tab.disabled) onChange(tab.value);
            }}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}
