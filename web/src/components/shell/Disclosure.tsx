import { useId, type ReactNode } from "react";
import { isOpen, setOpen, useUi } from "../../store/ui";

interface Props {
  /** Stable id; the open state is persisted under it. */
  id: string;
  title: string;
  /** Shown beside the title while collapsed and expanded: the one line an operator needs without opening. */
  summary?: ReactNode;
  defaultOpen?: boolean;
  className?: string;
  children: ReactNode;
}

/**
 * Collapsible section. The header is a real button (Enter/Space toggles, aria-expanded says which way),
 * the body is `inert` while closed so hidden controls are neither focusable nor clickable.
 * Height animates through grid-template-rows and is disabled under prefers-reduced-motion.
 */
export function Disclosure({ id, title, summary, defaultOpen = false, className = "", children }: Props) {
  // Subscribe to the open map so toggles re-render this section.
  useUi((s) => s.open[id]);
  const open = isOpen(id, defaultOpen);
  const bodyId = useId();
  return (
    <section className={`panel disclosure ${className}`} data-open={open} data-disclosure={id}>
      <h2 className="disclosure-head">
        <button
          type="button"
          className="disclosure-btn"
          aria-expanded={open}
          aria-controls={bodyId}
          onClick={() => setOpen(id, !open)}
        >
          <span className="disclosure-chevron" aria-hidden="true" />
          <span className="disclosure-title">{title}</span>
        </button>
        {summary && <div className="disclosure-summary">{summary}</div>}
      </h2>
      <div className="disclosure-body" id={bodyId} inert={!open}>
        <div className="disclosure-inner">{children}</div>
      </div>
    </section>
  );
}
