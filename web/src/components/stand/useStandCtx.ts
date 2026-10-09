import { useUi } from "../../store/ui";
import type { StandCtx } from "../../lib/interlocks";

/** Everything the stand interlocks need, from the low-rate store. */
export function useStandCtx(): StandCtx {
  const ws = useUi((s) => s.ws);
  const standLink = useUi((s) => s.link?.stand ?? null);
  const standSource = useUi((s) => s.standSource);
  const status = useUi((s) => s.standStatus);
  return {
    ws,
    configured: standLink !== null,
    linkUp: standLink?.connected ?? false,
    standSource,
    mode: status?.mode ?? null,
    actuationOk: status?.actuation_link_ok ?? false,
    loadcellOk: status?.loadcell_link_ok ?? false,
  };
}
