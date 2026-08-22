"use client";

/**
 * NavTree — 可复用导航树组件（React Bits 文档侧栏语法）。
 * 章法：mono 小节标签 + 左侧树线导轨 + 条目缩进；
 * hover 浮起纸凹底，active 在导轨上落 2px 墨标。
 * collapsed=true 时收成图标轨（标签/树线/徽标隐去，图标居中）。
 */
import { cn } from "@/lib/cn";
import Link from "next/link";
import type { ReactNode } from "react";

export type NavTreeItem = {
  id: string;
  label: string;
  icon?: ReactNode;
  /** 形如 NEW 的小徽标 */
  badge?: string;
  href?: string;
  external?: boolean;
  onClick?: () => void;
  active?: boolean;
};

export type NavTreeSection = {
  id: string;
  label?: string;
  /** 小节说明文字（非链接，如空态提示） */
  note?: string;
  items: NavTreeItem[];
};

type NavTreeProps = {
  sections: NavTreeSection[];
  collapsed?: boolean;
  /** 条目点击后回调（抽屉场景用于收起） */
  onNavigate?: () => void;
  /** nav 的无障碍名称，默认 "sections" */
  ariaLabel?: string;
  className?: string;
};

const itemBase =
  "group relative flex items-center gap-2.5 rounded-lg py-1.5 text-xs whitespace-nowrap transition-[background-color,color] duration-250 ease-fluid";

function Item({
  item,
  collapsed,
  onNavigate,
}: {
  item: NavTreeItem;
  collapsed: boolean;
  onNavigate?: () => void;
}): ReactNode {
  const cls = cn(
    itemBase,
    collapsed ? "justify-center px-0" : "px-3",
    item.active
      ? "bg-seal/8 font-medium text-seal"
      : "text-ink-soft hover:bg-seal/6 hover:text-seal",
  );
  const inner = (
    <>
      {/* active 朱砂标落在树线导轨上 */}
      {item.active && !collapsed ? (
        <span
          aria-hidden
          className="absolute top-1/2 -left-[13px] h-4 w-0.5 -translate-y-1/2 rounded-full bg-seal"
        />
      ) : null}
      {item.icon ? (
        <span
          aria-hidden
          className={cn(
            "flex shrink-0 items-center transition-colors duration-250",
            item.active ? "text-seal" : "text-ink-soft/70 group-hover:text-seal",
          )}
        >
          {item.icon}
        </span>
      ) : null}
      <span className={cn(collapsed && "sr-only")}>{item.label}</span>
      {item.badge && !collapsed ? (
        <span className="rounded-pill border border-ink/15 px-1.5 font-mono text-xs leading-4 text-ink-soft">
          {item.badge}
        </span>
      ) : null}
    </>
  );

  const handleClick = () => {
    item.onClick?.();
    onNavigate?.();
  };

  if (item.href && item.external) {
    return (
      <a
        href={item.href}
        target="_blank"
        rel="noreferrer"
        className={cls}
        title={collapsed ? item.label : undefined}
        onClick={handleClick}
      >
        {inner}
      </a>
    );
  }
  if (item.href) {
    return (
      <Link
        href={item.href}
        className={cls}
        title={collapsed ? item.label : undefined}
        aria-current={item.active ? "page" : undefined}
        onClick={handleClick}
      >
        {inner}
      </Link>
    );
  }
  return (
    <button
      type="button"
      className={cn(cls, "w-full cursor-pointer text-left")}
      title={collapsed ? item.label : undefined}
      aria-current={item.active ? "page" : undefined}
      onClick={handleClick}
    >
      {inner}
    </button>
  );
}

export function NavTree({
  sections,
  collapsed = false,
  onNavigate,
  ariaLabel = "sections",
  className,
}: NavTreeProps): ReactNode {
  return (
    <nav
      aria-label={ariaLabel}
      className={cn(
        "flex flex-col gap-6 overflow-x-hidden",
        collapsed ? "items-center px-2" : "px-4",
        className,
      )}
    >
      {sections.map((section) => (
        <div key={section.id} className={cn(collapsed && "w-full")}>
          {section.label && !collapsed ? (
            <p className="tracking-label mb-2 px-3 font-mono text-xs whitespace-nowrap text-ink-soft/80 uppercase">
              {section.label}
            </p>
          ) : null}
          {collapsed ? (
            <div aria-hidden className="mx-auto mb-3 h-px w-6 bg-ink/10" />
          ) : null}
          {section.note && !collapsed ? (
            <p className="ml-1.5 border-l border-ink/10 pl-3 text-xs whitespace-nowrap text-ink-soft/60">
              <span className="block px-3 py-0.5">{section.note}</span>
            </p>
          ) : null}
          <ul
            className={cn(
              "flex flex-col gap-0.5",
              !collapsed && "ml-1.5 border-l border-ink/10 pl-3",
            )}
          >
            {section.items.map((item) => (
              <li key={item.id}>
                <Item item={item} collapsed={collapsed} onNavigate={onNavigate} />
              </li>
            ))}
          </ul>
        </div>
      ))}
    </nav>
  );
}
