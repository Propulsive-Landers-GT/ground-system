import { useCallback, useEffect, useRef, type ReactNode } from "react";

interface Props {
  onConfirm: () => void;
  disabled?: boolean;
  /** Why the button is disabled; shown as a tooltip and read by screen readers. */
  why?: string;
  holdMs?: number;
  className?: string;
  children: ReactNode;
  sub?: ReactNode;
}

/**
 * Press-and-hold confirm. Works with pointer and with Space/Enter held down.
 * Releasing early cancels. The fill is driven through a CSS variable on the element
 * so the hold animation never goes through React state.
 */
export function HoldButton({ onConfirm, disabled, why, holdMs = 1000, className = "", children, sub }: Props) {
  const ref = useRef<HTMLButtonElement>(null);
  const raf = useRef(0);
  const t0 = useRef(0);
  const holding = useRef(false);
  const confirm = useRef(onConfirm);
  confirm.current = onConfirm;

  const setP = (p: number) => {
    const el = ref.current;
    if (!el) return;
    el.style.setProperty("--hold", String(p));
    el.dataset.holding = p > 0 && p < 1 ? "true" : "false";
  };

  const cancel = useCallback(() => {
    if (!holding.current) return;
    holding.current = false;
    cancelAnimationFrame(raf.current);
    setP(0);
  }, []);

  const start = useCallback(() => {
    if (holding.current || disabled) return;
    holding.current = true;
    t0.current = performance.now();
    const step = () => {
      if (!holding.current) return;
      const p = Math.min(1, (performance.now() - t0.current) / holdMs);
      setP(p);
      if (p >= 1) {
        holding.current = false;
        const el = ref.current;
        if (el) {
          el.dataset.fired = "true";
          window.setTimeout(() => {
            if (el) {
              el.dataset.fired = "false";
              el.style.setProperty("--hold", "0");
            }
          }, 350);
        }
        confirm.current();
        return;
      }
      raf.current = requestAnimationFrame(step);
    };
    raf.current = requestAnimationFrame(step);
  }, [disabled, holdMs]);

  useEffect(() => {
    if (disabled) cancel();
  }, [disabled, cancel]);
  useEffect(() => () => cancelAnimationFrame(raf.current), []);

  return (
    <button
      ref={ref}
      type="button"
      className={`btn hold ${className}`}
      aria-disabled={disabled || undefined}
      data-disabled={disabled || undefined}
      title={disabled ? why : "Press and hold for 1 second"}
      aria-description={disabled ? why : "Press and hold for one second to confirm"}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        start();
      }}
      onPointerUp={cancel}
      onPointerCancel={cancel}
      onLostPointerCapture={cancel}
      onKeyDown={(e) => {
        if ((e.key === " " || e.key === "Enter") && !e.repeat) {
          e.preventDefault();
          start();
        } else if (e.key === "Escape") cancel();
      }}
      onKeyUp={(e) => {
        if (e.key === " " || e.key === "Enter") {
          e.preventDefault();
          cancel();
        }
      }}
      onBlur={cancel}
      onContextMenu={(e) => e.preventDefault()}
      onClick={(e) => e.preventDefault()}
    >
      <span className="hold-fill" aria-hidden="true" />
      <span className="hold-label">{children}</span>
      {sub && <span className="hold-sub">{sub}</span>}
    </button>
  );
}
