// Source: https://www.beautifului.dev/ (Task Rows), retrieved 2026-10-19.
// Copyright (c) 2026 Shane Levine. MIT; see LICENSE.
"use client";

import { useState, type ReactNode } from "react";

/* Adapted from Beautiful UI Task Rows. Project states are data-driven. */

function SpinnerRing({ active, children }: { active?: boolean; children?: ReactNode }) {
  const size = 24, stroke = 2;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  return (
    <span className="relative inline-flex shrink-0 items-center justify-center" style={{ width: size, height: size }}>
      <svg
        width={size} height={size} className="absolute inset-0"
        style={active ? { animation: "spin 1.1s linear infinite" } : undefined}
      >
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--line)" strokeWidth={stroke} />
        {active && (
          <circle
            cx={size / 2} cy={size / 2} r={r} fill="none"
            stroke="var(--ink-3)" strokeWidth={stroke} strokeLinecap="round"
            strokeDasharray={`${c * 0.28} ${c * 0.72}`}
          />
        )}
      </svg>
      <span className="relative text-[10.5px] font-semibold tabular-nums text-ink">{children}</span>
    </span>
  );
}

function Badge({ tone, children }: { tone: "red" | "green"; children: ReactNode }) {
  return (
    <span
      className={`flex size-5.5 shrink-0 items-center justify-center rounded-full text-white
        ${tone === "red" ? "bg-red" : "bg-green"}`}
      style={{ animation: "pop-in 300ms cubic-bezier(0.23,1,0.32,1) both" }}
    >
      {children}
    </span>
  );
}

const XIcon = (
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3.5" strokeLinecap="round"><path d="M18 6L6 18M6 6l12 12" /></svg>
);
const CheckIcon = (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3.5" strokeLinecap="round" strokeLinejoin="round"><path d="M20 6L9 17l-5-5" /></svg>
);

/* One detail line shown when a task row is expanded. */
export type TaskDetail = { label: string; meta: string };

/* A task or project row with an explicit, externally supplied state. */
export type TaskRow = {
  key: string;
  label: string;
  amount: string;
  status: "done" | "running" | "idle" | "unknown" | "failed";
  icon?: ReactNode;
  path?: string;
  caption?: string;
  metadata?: ReactNode;
  content?: ReactNode;
  step?: number;
  details: TaskDetail[];
};

export type TaskRowsLabels = {
  completed: string;
  failed: string;
};

const DEFAULT_LABELS: TaskRowsLabels = {
  completed: "Completed",
  failed: "Failed",
};

export default function TaskRows({
  variant = "Capsules",
  rows,
  labels,
  className,
  onToggleRow,
  onActivateRow,
  renderActions,
}: {
  variant?: string;
  rows: TaskRow[];
  labels?: Partial<TaskRowsLabels>;
  className?: string;
  onToggleRow?: (key: string, open: boolean) => void;
  onActivateRow?: (row: TaskRow) => void;
  renderActions?: (row: TaskRow) => ReactNode;
}) {
  const [manualOpen, setManualOpen] = useState<Record<string, boolean>>({});
  const copy = { ...DEFAULT_LABELS, ...labels };

  const badgeFor = (row: TaskRow) => {
    if (row.icon) return row.icon;
    if (row.status === "done") return <Badge tone="green">{CheckIcon}</Badge>;
    if (row.status === "failed") return <Badge tone="red">{XIcon}</Badge>;
    return <SpinnerRing active={row.status === "running"}>{row.step}</SpinnerRing>;
  };

  const pillFor = (row: TaskRow) => {
    if (row.status !== "done" && row.status !== "failed") return null;
    return <span className={`inline-flex h-5.5 items-center rounded-full px-2 text-[11.5px] font-medium ${row.status === "done" ? "bg-green-tint text-green" : "bg-red-tint text-red"}`}>{row.status === "done" ? copy.completed : copy.failed}</span>;
  };

  const list = variant === "List";
  const cards = variant === "Cards";
  return (
    <div
      className={`bui-task-rows flex w-full flex-col ${
        list ? "gap-0 self-start rounded-card bg-surface shadow-card" : "min-h-[196px] gap-2"
      }${className ? ` ${className}` : ""}`}
    >
      {rows.map((row, i) => {
        const open = manualOpen[row.key] ?? false;
        const detailsId = `project-details-${encodeURIComponent(row.key)}`;
        return (
          <div
            key={row.key}
            data-status={row.status}
            onClick={onActivateRow ? event => { if (!(event.target as Element).closest('button, a, input, select, textarea')) onActivateRow(row); } : undefined}
            className={`bui-task-row self-stretch transition-[border-radius,background-color] duration-300 hover:bg-inset ${
              list ? "border-b border-line last:border-0" : "bg-surface shadow-card"
            }`}
            style={{
              borderRadius: list ? 0 : open ? 14 : 22,
              animation: `fade-up 450ms cubic-bezier(0.23,1,0.32,1) ${i * 80}ms backwards`,
            }}
          >
            <div className="bui-task-heading flex items-center gap-1 pr-2">
            <button
              type="button"
              aria-expanded={onActivateRow ? undefined : open}
              aria-controls={onActivateRow ? undefined : detailsId}
              aria-label={`${row.label} ${onActivateRow ? '查看运行输出' : '项目详情'}`}
              onClick={() => {
                if (onActivateRow) { onActivateRow(row); return; }
                setManualOpen((current) => ({ ...current, [row.key]: !open }));
                onToggleRow?.(row.key, !open);
              }}
              className="bui-task-toggle flex min-h-14 min-w-0 flex-1 items-center gap-2.5 px-2.5 py-2.5 text-left"
            >
              <span className="flex size-8 shrink-0 items-center justify-center">
                {badgeFor(row)}
              </span>
              <span className="bui-task-label min-w-0 flex-1 text-[13px] font-medium text-ink">
                <span className="block truncate" title={row.label}>{row.label}</span>
                {cards && row.metadata && <span className="bui-task-summary">{row.metadata}</span>}
                {row.path && <span className="mt-1 block truncate font-mono text-[11px] font-normal text-ink-3" title={row.path}>{row.path}</span>}
                {row.caption && <span className="bui-task-caption mt-1 flex items-center gap-1.5 text-[10px] font-normal text-ink-2"><span className="size-1 rounded-full bg-ink-3" />{row.caption}</span>}
              </span>
              {!cards && <span className="bui-task-amount text-[11px] text-ink-2 tabular-nums">{row.amount}</span>}
              {pillFor(row)}
              {!onActivateRow && <span
                aria-hidden="true"
                className="-ml-2 flex size-7 shrink-0 items-center justify-center rounded-full text-ink-3"
              >
                <svg
                  width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"
                  className="transition-transform duration-300"
                  style={{ transform: open ? "rotate(180deg)" : "rotate(0)" }}
                >
                  <path d="M6 9l6 6 6-6" />
                </svg>
              </span>}
            </button>
            {renderActions?.(row)}
            {onActivateRow && <button type="button" className="bui-task-expand" aria-label={`${row.label} 展开项目详情`} title="展开 / 收起项目详情" aria-expanded={open} aria-controls={detailsId} onClick={() => { setManualOpen(current => ({ ...current, [row.key]: !open })); onToggleRow?.(row.key, !open); }}><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ transform: open ? 'rotate(180deg)' : undefined }}><path d="M6 9l6 6 6-6" /></svg></button>}
            </div>

            {row.content}

            {/* dropdown detail — same expandable grammar as Chain of Thought */}
            <div
              id={detailsId}
              aria-hidden={!open}
              className="grid transition-[grid-template-rows,opacity] duration-300"
                style={{
                  gridTemplateRows: open ? "1fr" : "0fr",
                  opacity: open ? 1 : 0,
                  transitionTimingFunction: "cubic-bezier(0.23, 1, 0.32, 1)",
                }}
              >
                <div className="overflow-hidden">
                  <div className="mb-2.5 grid grid-cols-[24px_1fr] gap-2.5 px-2.5">
                    <span aria-hidden className="mx-auto h-full w-px bg-line" />
                    <div className="flex flex-col gap-1.5">
                      {row.details.map((d, j) => (
                        <div
                          key={d.label}
                          className="flex min-w-0 flex-wrap items-center justify-between gap-2"
                          style={
                            open
                              ? { animation: `fade-up 300ms cubic-bezier(0.23,1,0.32,1) ${120 + j * 100}ms both` }
                              : undefined
                          }
                        >
                          <span className="bui-task-detail-label break-all text-[12px] text-ink-2">{d.label}</span>
                          <span className="bui-task-detail-value break-all font-mono text-[11.5px] text-ink-3 tabular-nums">
                            {d.meta}
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
              </div>
          </div>
        );
      })}
    </div>
  );
}
