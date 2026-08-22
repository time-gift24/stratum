"use client";

/**
 * 动效基础设施：reduced-motion 上下文 + 全站统一的 variants / 时长 / 缓动。
 * 所有编排式动画从这里取尺度，唯一的门控是 prefers-reduced-motion。
 */
import { motion, type MotionProps, type Variants } from "motion/react";
import {
  createContext,
  useContext,
  useSyncExternalStore,
  type ReactNode,
} from "react";

function subscribeToReducedMotion(callback: () => void): () => void {
  const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  mediaQuery.addEventListener("change", callback);
  return () => mediaQuery.removeEventListener("change", callback);
}

function getReducedMotionSnapshot(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function getReducedMotionServerSnapshot(): boolean {
  return false;
}

const ReducedMotionContext = createContext<boolean>(false);

export function useReducedMotion(): boolean {
  return useContext(ReducedMotionContext);
}

export function ReducedMotionProvider({
  children,
}: {
  children: ReactNode;
}): ReactNode {
  const prefersReducedMotion = useSyncExternalStore(
    subscribeToReducedMotion,
    getReducedMotionSnapshot,
    getReducedMotionServerSnapshot,
  );

  return (
    <ReducedMotionContext.Provider value={prefersReducedMotion}>
      {children}
    </ReducedMotionContext.Provider>
  );
}

/** 流体世界的缓动：快起缓落，像墨入水。 */
export const easeFluid = [0.22, 1, 0.36, 1] as const;
/** 下潜：进出皆加速。 */
export const easeDive = [0.65, 0, 0.35, 1] as const;

export const duration = {
  reveal: 0.7,
  hover: 0.25,
  drift: 14,
} as const;

export const fadeUp: Variants = {
  hidden: { opacity: 0, y: 28 },
  visible: {
    opacity: 1,
    y: 0,
    transition: { duration: duration.reveal, ease: [...easeFluid] },
  },
};

export const fadeIn: Variants = {
  hidden: { opacity: 0 },
  visible: {
    opacity: 1,
    transition: { duration: duration.reveal, ease: [...easeFluid] },
  },
};

export const stagger: Variants = {
  hidden: {},
  visible: { transition: { staggerChildren: 0.12 } },
};

const reducedVariants: Variants = {
  hidden: { opacity: 0 },
  visible: { opacity: 1, transition: { duration: 0.01 } },
};

type RevealProps = {
  children: ReactNode;
  className?: string;
  /** 视口内触发的阈值与回缩量 */
  amount?: number;
} & Omit<MotionProps, "variants" | "initial" | "whileInView">;

/** 滚动入场编排包装：进入视口时 fadeUp，离开不回放。 */
export function Reveal({
  children,
  className,
  amount = 0.35,
  ...props
}: RevealProps): ReactNode {
  const prefersReducedMotion = useReducedMotion();

  return (
    <motion.div
      initial="hidden"
      whileInView="visible"
      viewport={{ once: true, amount }}
      variants={prefersReducedMotion ? reducedVariants : fadeUp}
      className={className}
      {...props}
    >
      {children}
    </motion.div>
  );
}
