import { type ReactNode, useEffect, useRef } from "react";
import { cn } from "@/lib/utils";

/** A modal on the platform's <dialog>: focus kept inside, Esc closes, the page behind inert. A
 * click on the backdrop closes it too. */
export function Dialog({
  open,
  onClose,
  label,
  className,
  children,
}: {
  open: boolean;
  onClose: () => void;
  label: string;
  className?: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (open && !dialog?.open) dialog?.showModal();
    if (!open && dialog?.open) dialog.close();
  }, [open]);
  return (
    <dialog
      ref={ref}
      aria-label={label}
      onClose={onClose}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      className={cn(
        "m-auto max-h-[80vh] w-[min(640px,calc(100vw-32px))] overflow-hidden rounded-xl border border-line bg-raised p-0 text-foreground shadow-xl backdrop:bg-black/40",
        className,
      )}
    >
      {open && children}
    </dialog>
  );
}
