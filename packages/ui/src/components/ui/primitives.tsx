import type { ButtonHTMLAttributes, HTMLAttributes, ReactNode } from "react";

type Tone = "neutral" | "success" | "warning" | "danger" | "info";

const toneClassNames: Record<Tone, string> = {
  neutral: "border-border bg-panel text-foreground",
  success: "border-success/30 bg-success/10 text-success",
  warning: "border-warning/30 bg-warning/10 text-warning",
  danger: "border-danger/30 bg-danger/10 text-danger",
  info: "border-info/30 bg-info/10 text-info",
};

export function Panel({
  children,
  className = "",
  ...props
}: HTMLAttributes<HTMLElement> & { children: ReactNode }) {
  return (
    <section
      className={`min-w-0 rounded-lg border border-border bg-panel shadow-panel ${className}`}
      {...props}
    >
      {children}
    </section>
  );
}

export function PanelHeader({
  title,
  description,
  action,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex min-h-14 items-start justify-between gap-4 border-b border-border px-4 py-3">
      <div className="min-w-0">
        <h2 className="truncate text-sm font-semibold text-foreground">{title}</h2>
        {description ? <p className="mt-1 text-xs text-muted">{description}</p> : null}
      </div>
      {action ? <div className="shrink-0">{action}</div> : null}
    </div>
  );
}

export function Button({
  variant = "secondary",
  className = "",
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
}) {
  const variantClassNames = {
    primary: "border-primary bg-primary text-white hover:bg-primary/90",
    secondary: "border-border bg-surface text-foreground hover:bg-background",
    ghost: "border-transparent bg-transparent text-muted hover:bg-surface hover:text-foreground",
    danger: "border-danger/40 bg-danger/10 text-danger hover:bg-danger/15",
  };

  return (
    <button
      className={`inline-flex h-9 items-center justify-center gap-2 rounded-md border px-3 text-sm font-medium transition-colors duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:cursor-not-allowed disabled:opacity-45 ${variantClassNames[variant]} ${className}`}
      type="button"
      {...props}
    >
      {children}
    </button>
  );
}

export function IconButton({
  label,
  children,
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; children: ReactNode }) {
  return (
    <button
      aria-label={label}
      title={label}
      className={`inline-flex h-9 w-9 items-center justify-center rounded-md border border-border bg-surface text-muted transition-colors duration-200 hover:bg-background hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:cursor-not-allowed disabled:opacity-45 ${className}`}
      type="button"
      {...props}
    >
      {children}
    </button>
  );
}

export function Badge({ tone = "neutral", children }: { tone?: Tone; children: ReactNode }) {
  return (
    <span
      className={`inline-flex h-6 items-center rounded-md border px-2 text-xs font-medium ${toneClassNames[tone]}`}
    >
      {children}
    </span>
  );
}

export function Field({
  label,
  value,
  children,
}: {
  label: string;
  value?: string;
  children?: ReactNode;
}) {
  return (
    <label className="grid gap-1 text-xs font-medium text-muted">
      <span>{label}</span>
      {children ?? (
        <input
          className="h-9 rounded-md border border-border bg-surface px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          defaultValue={value}
        />
      )}
    </label>
  );
}

export function DataTable({
  columns,
  children,
}: {
  columns: string[];
  children: ReactNode;
}) {
  return (
    <div className="max-w-full overflow-x-auto">
      <table className="w-full min-w-[920px] border-separate border-spacing-0 text-left text-sm">
        <thead>
          <tr>
            {columns.map((column) => (
              <th
                className="border-b border-border px-4 py-3 text-xs font-semibold uppercase text-muted"
                key={column}
                scope="col"
              >
                {column}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>{children}</tbody>
      </table>
    </div>
  );
}

export function TableCell({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return <td className={`border-b border-border px-4 py-3 align-middle ${className}`}>{children}</td>;
}

export function EmptyState({ title, description }: { title: string; description: string }) {
  return (
    <div className="rounded-lg border border-dashed border-border bg-surface px-4 py-6 text-sm">
      <p className="font-medium text-foreground">{title}</p>
      <p className="mt-1 text-muted">{description}</p>
    </div>
  );
}
