"use client";

/**
 * App 侧栏：NavTree 组件驱动。
 * 布局：顶部品牌位（logo 资产待补，先用「筹」字章占位）+ 对话区右上方收起按钮（纯图标）；
 * 对话/历史在中部滚动区，资源钉在最底部；桌面可折叠为图标轨（localStorage 记忆），
 * lg 以下收纳为抽屉。
 */
import { NavTree, type NavTreeSection } from "@/components/ui/nav-tree";
import { cn } from "@/lib/cn";
import { siteConfig } from "@/lib/config";
import { useLanguage } from "@/lib/i18n";
import {
  Blocks,
  Github,
  MessageSquarePlus,
  PanelLeft,
  X,
} from "lucide-react";
import { useEffect, useRef, useState, type ReactNode, type RefObject } from "react";

const COLLAPSE_KEY = "stratum-site-sidebar-collapsed";

function useMainSections(): NavTreeSection[] {
  const { t } = useLanguage();
  return [
    {
      id: "conversation",
      label: t.conversation.title,
      items: [
        {
          id: "new",
          label: t.conversation.newChat,
          icon: <MessageSquarePlus size={15} />,
          href: "/conversation",
          active: true,
        },
      ],
    },
    {
      id: "history",
      label: t.conversation.history,
      note: t.conversation.historyEmpty,
      items: [],
    },
  ];
}

function useResourceSections(): NavTreeSection[] {
  const { t } = useLanguage();
  return [
    {
      id: "resources",
      label: t.conversation.resources,
      items: [
        {
          id: "gallery",
          label: t.footer.gallery,
          icon: <Blocks size={15} />,
          href: "/components",
        },
        {
          id: "github",
          label: "GitHub",
          icon: <Github size={15} />,
          href: siteConfig.githubUrl,
          external: true,
        },
      ],
    },
  ];
}

/** 品牌位。TODO(assets): 替换为正式 Stratum 标志（仓库中 SVG 已佚失）。 */
function BrandMark(): ReactNode {
  return (
    <span
      aria-hidden
      className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-ink font-display text-xs font-bold text-paper"
    >
      筹
    </span>
  );
}

function CollapseButton({
  collapsed,
  onToggle,
}: {
  collapsed: boolean;
  onToggle: () => void;
}): ReactNode {
  const { t } = useLanguage();
  return (
    <button
      type="button"
      onClick={onToggle}
      aria-label={collapsed ? t.conversation.expand : t.conversation.collapse}
      title={collapsed ? t.conversation.expand : t.conversation.collapse}
      className="flex h-9 w-9 shrink-0 cursor-pointer items-center justify-center rounded-lg text-ink-soft transition-colors duration-250 hover:bg-seal/6 hover:text-seal"
    >
      <PanelLeft size={16} aria-hidden />
    </button>
  );
}

export function AppSidebar({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}): ReactNode {
  const mainSections = useMainSections();
  const resourceSections = useResourceSections();
  const { t } = useLanguage();
  const [collapsed, setCollapsed] = useState(false);
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- 挂载后从 localStorage 恢复折叠状态，避免 SSR 水合不一致
    setCollapsed(window.localStorage.getItem(COLLAPSE_KEY) === "1");
  }, []);

  // 抽屉打开：焦点移到关闭钮，Escape 关闭
  useEffect(() => {
    if (!open) return;
    closeRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  const toggleCollapsed = () => {
    // localStorage 写入放在事件处理器里，state updater 保持纯净（StrictMode 会双调 updater）
    const next = !collapsed;
    window.localStorage.setItem(COLLAPSE_KEY, next ? "1" : "0");
    setCollapsed(next);
  };

  return (
    <>
      {/* 桌面：静态栏，可折叠为图标轨，内容独立滚动；右侧分割线不满高（上下留呼吸） */}
      <aside
        className={cn(
          "sticky top-20 hidden h-[calc(100svh-5rem)] shrink-0 flex-col pt-4 pb-8 transition-[width] duration-300 ease-fluid lg:flex",
          collapsed ? "w-16" : "w-60",
        )}
      >
        <span
          aria-hidden
          className="absolute top-2 right-0 bottom-10 w-px bg-ink/10"
        />
        <div
          className={cn(
            "mb-3 flex items-center",
            collapsed ? "flex-col gap-2.5 px-2" : "justify-between px-4",
          )}
        >
          <BrandMark />
          <CollapseButton collapsed={collapsed} onToggle={toggleCollapsed} />
        </div>
        <div className="flex-1 overflow-y-auto">
          <NavTree
            sections={mainSections}
            collapsed={collapsed}
            ariaLabel={t.conversation.navAria}
          />
        </div>
        <div className="shrink-0 pt-3">
          <NavTree
            sections={resourceSections}
            collapsed={collapsed}
            ariaLabel={t.conversation.navAria}
          />
        </div>
      </aside>

      {/* 移动/中屏：抽屉（dialog；不加 inert——结构成本高，记为已知取舍） */}
      {open ? (
        <div className="fixed inset-0 z-50 lg:hidden">
          <div
            aria-hidden
            onClick={onClose}
            className="absolute inset-0 bg-ink/30 backdrop-blur-sm"
          />
          <aside
            role="dialog"
            aria-modal="true"
            aria-label={t.conversation.menu}
            className="shadow-card absolute inset-y-0 left-0 flex w-72 flex-col bg-paper"
          >
            <div className="flex items-center justify-between px-4 py-4">
              <BrandMark />
              <CloseButton onClose={onClose} closeRef={closeRef} />
            </div>
            <div className="flex-1 overflow-y-auto overscroll-contain">
              <NavTree
                sections={mainSections}
                onNavigate={onClose}
                ariaLabel={t.conversation.navAria}
              />
            </div>
            <div className="shrink-0 py-3">
              <NavTree
                sections={resourceSections}
                onNavigate={onClose}
                ariaLabel={t.conversation.navAria}
              />
            </div>
          </aside>
        </div>
      ) : null}
    </>
  );
}

function CloseButton({
  onClose,
  closeRef,
}: {
  onClose: () => void;
  closeRef?: RefObject<HTMLButtonElement | null>;
}): ReactNode {
  const { t } = useLanguage();
  return (
    <button
      ref={closeRef}
      type="button"
      onClick={onClose}
      aria-label={t.conversation.menu}
      className="cursor-pointer p-2 text-ink-soft transition-colors hover:text-ink"
    >
      <X size={18} aria-hidden />
    </button>
  );
}
