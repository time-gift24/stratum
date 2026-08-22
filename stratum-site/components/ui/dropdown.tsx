"use client";

/**
 * Dropdown — 可复用列表选择器（交互骨架参照 React Bits Pro Prompt Input 2）。
 * 完整键盘导航：↑↓/Home/End/Enter/Space/Esc/Tab，焦点在菜单与触发器间正确往返。
 * 样式消费纸墨 token：paper 地、card 阴影、seal 选中勾。
 */
import { cn } from "@/lib/cn";
import { Check, ChevronDown } from "lucide-react";
import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";

export type DropdownOption = {
  name: string;
  hint?: string;
};

type DropdownProps = {
  label: string;
  options: DropdownOption[];
  value: string;
  onChange: (next: string) => void;
  icon?: ReactNode;
  triggerMaxWidth?: string;
};

export function Dropdown({
  label,
  options,
  value,
  onChange,
  icon,
  triggerMaxWidth,
}: DropdownProps): ReactNode {
  const [open, setOpen] = useState(false);
  const [shown, setShown] = useState(false);
  const [active, setActive] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
        setShown(false);
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  useEffect(() => {
    if (open) menuRef.current?.focus({ preventScroll: true });
  }, [open]);

  useEffect(() => () => cancelAnimationFrame(frameRef.current ?? 0), []);

  const openMenu = (index: number, withAnimation: boolean) => {
    setActive(index);
    setOpen(true);
    if (withAnimation) {
      setShown(false);
      frameRef.current = requestAnimationFrame(() => setShown(true));
    } else {
      setShown(true);
    }
  };

  const close = (restoreFocus = true) => {
    // 取消未决的展开帧，避免关闭后被陈旧 setShown(true) 覆写
    if (frameRef.current !== undefined) {
      cancelAnimationFrame(frameRef.current);
      frameRef.current = undefined;
    }
    setOpen(false);
    setShown(false);
    if (restoreFocus) triggerRef.current?.focus({ preventScroll: true });
  };

  const select = (index: number) => {
    onChange(options[index].name);
    close();
  };

  const onTriggerKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      openMenu(0, false);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      openMenu(options.length - 1, false);
    }
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        close();
        break;
      case "ArrowDown":
        event.preventDefault();
        setActive((i) => (i + 1) % options.length);
        break;
      case "ArrowUp":
        event.preventDefault();
        setActive((i) => (i - 1 + options.length) % options.length);
        break;
      case "Home":
        event.preventDefault();
        setActive(0);
        break;
      case "End":
        event.preventDefault();
        setActive(options.length - 1);
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        select(active);
        break;
      case "Tab":
        close(false);
        break;
    }
  };

  return (
    <div ref={rootRef} className="relative">
      {open ? (
        <div
          ref={menuRef}
          role="listbox"
          aria-label={label}
          tabIndex={-1}
          onKeyDown={onMenuKeyDown}
          className={cn(
            "rounded-card bg-paper shadow-card absolute bottom-full left-0 z-30 mb-2 w-64 origin-bottom-left overflow-hidden p-1.5 transition-[opacity,transform] duration-250 ease-fluid outline-none motion-reduce:transition-none",
            shown ? "scale-100 opacity-100" : "scale-95 opacity-0",
          )}
        >
          {options.map((item, index) => {
            const selected = item.name === value;
            return (
              <button
                key={item.name}
                type="button"
                role="option"
                aria-selected={selected}
                tabIndex={-1}
                onClick={() => select(index)}
                onPointerEnter={() => setActive(index)}
                className={cn(
                  "flex w-full cursor-pointer items-center gap-2.5 rounded-xl px-3 py-2 text-left transition-colors duration-250",
                  index === active ? "bg-paper-dim" : "bg-transparent",
                )}
              >
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-medium text-ink">
                    {item.name}
                  </span>
                  {item.hint ? (
                    <span className="block truncate text-xs text-ink-soft">
                      {item.hint}
                    </span>
                  ) : null}
                </span>
                {selected ? (
                  <Check size={14} className="shrink-0 text-seal" aria-hidden />
                ) : null}
              </button>
            );
          })}
        </div>
      ) : null}

      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`${label}: ${value}`}
        onClick={() => (open ? close() : openMenu(0, true))}
        onKeyDown={onTriggerKeyDown}
        className="rounded-pill shadow-chip hover:shadow-chip-hover inline-flex h-9 cursor-pointer items-center gap-1.5 bg-white px-3.5 text-sm font-medium whitespace-nowrap text-ink-soft transition-[background-color,color,transform] duration-250 ease-fluid hover:text-ink active:scale-97"
      >
        {icon}
        <span className={cn("truncate", triggerMaxWidth)}>
          {value || label}
        </span>
        <ChevronDown
          size={14}
          aria-hidden
          className={cn(
            "shrink-0 transition-transform duration-250",
            open && "rotate-180",
          )}
        />
      </button>
    </div>
  );
}
