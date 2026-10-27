import "./modal.css";
import { useEffect, useRef, useId, type ReactNode } from "react";
import { X } from "lucide-react";
import { IconButton } from "./primitives";

export function Modal({ title, children, onClose, className = "" }: {
  title: string; children: ReactNode; onClose: () => void; className?: string;
}) {
  const dialog = useRef<HTMLDialogElement>(null);const titleId=useId();
  useEffect(() => {
    const element = dialog.current;
    const previous = document.activeElement as HTMLElement | null;
    element?.showModal();
    const first=element?.querySelector<HTMLElement>('[data-dialog-initial-focus],input:not([type=hidden]):not([type=checkbox]):not([readonly]):not(:disabled),textarea:not([readonly]):not(:disabled),select:not(:disabled)')??element?.querySelector<HTMLElement>('footer button:not(:disabled)')??element?.querySelector<HTMLElement>('h2');
    first?.focus({preventScroll:true});
    return () => {element?.close();const target=previous?.isConnected?previous:document.querySelector<HTMLElement>('[aria-current=page]');target?.focus({preventScroll:true});};
  }, []);
  return <dialog ref={dialog} className={`app-modal ${className}`} aria-label={title} aria-labelledby={titleId} aria-modal="true"
    onCancel={event => { event.preventDefault(); onClose(); }}
    onClick={event => {
      if (event.target !== event.currentTarget) return;
      const rect = event.currentTarget.getBoundingClientRect();
      if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) onClose();
    }}>
    <header className="flex items-center justify-between gap-4 border-b border-border px-5 py-4">
      <h2 id={titleId} tabIndex={-1} className="font-semibold">{title}</h2><IconButton label="닫기" onClick={onClose}><X size={18} /></IconButton>
    </header>{children}
  </dialog>;
}
