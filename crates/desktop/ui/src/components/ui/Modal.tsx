import { useEffect, useRef, type ReactNode } from "react";
import { X } from "lucide-react";
import { IconButton } from "./primitives";

export function Modal({ title, children, onClose, className = "" }: {
  title: string; children: ReactNode; onClose: () => void; className?: string;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    const previous = document.activeElement as HTMLElement | null;
    element?.showModal();
    return () => { element?.close(); previous?.focus(); };
  }, []);
  return <dialog ref={dialog} className={`app-modal ${className}`} aria-label={title}
    onCancel={event => { event.preventDefault(); onClose(); }}
    onClick={event => {
      if (event.target !== event.currentTarget) return;
      const rect = event.currentTarget.getBoundingClientRect();
      if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) onClose();
    }}>
    <header className="flex items-center justify-between gap-4 border-b border-border px-5 py-4">
      <h2 className="font-semibold">{title}</h2><IconButton label="닫기" onClick={onClose}><X size={18} /></IconButton>
    </header>{children}
  </dialog>;
}
